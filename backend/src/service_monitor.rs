//! Container health probes and their status history (F-39.1 / S-09.3).

use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::{Query, State, rejection::QueryRejection},
    http::{HeaderValue, StatusCode, header},
    routing::{get, post},
};
use chrono::{DateTime, TimeDelta, Utc};
use futures_util::future::join_all;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use tokio::sync::Mutex;
use utoipa::ToSchema;

use crate::{
    AppState, READY_DB_TIMEOUT,
    auth::RequireAdmin,
    config::ConfigError,
    error::{ApiError, ErrorBody},
    mutex::lock,
};

const DATABASE: &str = "database";
/// 背景探測間隔：預設 30 秒，可用 `SERVICE_PROBE_INTERVAL_SECS` 在 10 至 300 秒之間調整（S-09.3）。
pub const DEFAULT_PROBE_INTERVAL: Duration = Duration::from_secs(30);
const MIN_PROBE_INTERVAL_SECS: u64 = 10;
const MAX_PROBE_INTERVAL_SECS: u64 = 300;
const MANUAL_INTERVAL: Duration = Duration::from_secs(10);
const HISTORY_TIMEOUT: Duration = Duration::from_secs(2);
const RETENTION_DAYS: i64 = 30;
const RECENT_EVENTS: usize = 10;
const MAX_PENDING_EVENTS: usize = 100;

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
#[serde(untagged)]
enum ServiceText {
    Text(String),
    Translations(BTreeMap<String, String>),
}

impl ServiceText {
    fn has_text(&self) -> bool {
        match self {
            Self::Text(text) => !text.trim().is_empty(),
            Self::Translations(map) => {
                !map.is_empty() && map.values().all(|text| !text.trim().is_empty())
            }
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonitoredService {
    name: String,
    label: ServiceText,
    impact: ServiceText,
    #[serde(rename = "url")]
    endpoint: Url,
}

impl fmt::Debug for MonitoredService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MonitoredService")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl MonitoredService {
    pub fn parse(input: &str) -> Result<Vec<Self>, ConfigError> {
        if input.trim().is_empty() {
            return Ok(Vec::new());
        }
        let entries: Vec<serde_json::Value> = serde_json::from_str(input)
            .map_err(|_| ConfigError("MONITORED_SERVICES must be a JSON array".into()))?;
        let mut names = HashSet::from([DATABASE.to_string()]);
        entries
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                let error =
                    |field| ConfigError(format!("invalid MONITORED_SERVICES[{index}].{field}"));
                let mut service: Self = serde_json::from_value(value).map_err(|_| {
                    ConfigError(format!(
                        "invalid MONITORED_SERVICES[{index}] (expected name, label, url and impact)"
                    ))
                })?;
                if service.name.is_empty()
                    || !service
                        .name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                    || !names.insert(service.name.clone())
                {
                    return Err(error("name (invalid, duplicate or reserved)"));
                }
                if !service.label.has_text() {
                    return Err(error("label"));
                }
                if !service.impact.has_text() {
                    return Err(error("impact"));
                }
                let url = &mut service.endpoint;
                if !matches!(url.scheme(), "http" | "https") || !url.has_host() {
                    return Err(error("url (expected an HTTP(S) URL)"));
                }
                // reqwest interprets URL userinfo as Basic auth. Remove it before constructing a request.
                url.set_password(None).map_err(|()| error("url"))?;
                url.set_username("").map_err(|()| error("url"))?;
                url.set_fragment(None);
                Ok(service)
            })
            .collect()
    }

    fn target(&self) -> String {
        let host = self.endpoint.host_str().expect("validated service host");
        let path = self.endpoint.path();
        match self.endpoint.port() {
            Some(port) => format!("{host}:{port}{path}"),
            None => format!("{host}{path}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
enum Status {
    #[default]
    Checking,
    NoData,
    Healthy,
    Unhealthy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
enum ServiceFailure {
    ConnectionFailed,
    Timeout,
    #[serde(rename = "non_2xx")]
    #[sqlx(rename = "non_2xx")]
    Non2xx,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
enum EventStatus {
    Healthy,
    Unhealthy,
}

#[derive(Clone, FromRow, Serialize, ToSchema)]
struct StatusEvent {
    #[serde(skip)]
    service_name: String,
    status: EventStatus,
    occurred_at: DateTime<Utc>,
    failure: Option<ServiceFailure>,
}

#[derive(Clone, Default, Serialize, ToSchema)]
struct CurrentStatus {
    status: Status,
    failure: Option<ServiceFailure>,
    response_time_ms: Option<u64>,
    checked_at: Option<DateTime<Utc>>,
    last_healthy_at: Option<DateTime<Utc>>,
    status_since: Option<DateTime<Utc>>,
}

#[derive(Default)]
struct ProbeState {
    current: CurrentStatus,
    consecutive_failures: u8,
    first_failure_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Trigger {
    Scheduled,
    Manual,
}

struct ProbeResult {
    failure: Option<ServiceFailure>,
    response_time_ms: Option<u64>,
}

impl ProbeState {
    /// Only scheduled probes count toward the consecutive-failure rule, so repeated manual
    /// checks cannot shorten the debounce window. A manual failure still refreshes the probe data.
    fn observe(&mut self, result: ProbeResult, at: DateTime<Utc>, trigger: Trigger) {
        self.current.checked_at = Some(at);
        self.current.response_time_ms = result.response_time_ms;
        let next = if result.failure.is_none() {
            self.consecutive_failures = 0;
            self.first_failure_at = None;
            self.current.last_healthy_at = Some(at);
            Status::Healthy
        } else if trigger == Trigger::Manual {
            return;
        } else {
            self.consecutive_failures = self.consecutive_failures.saturating_add(1);
            self.first_failure_at.get_or_insert(at);
            match (self.current.status, self.consecutive_failures) {
                (_, 2..) => Status::Unhealthy,
                (Status::Checking, _) => Status::NoData,
                (status, _) => status,
            }
        };
        if next != self.current.status {
            self.current.status = next;
            self.current.status_since = Some(match next {
                Status::Unhealthy => self.first_failure_at.unwrap_or(at),
                _ => at,
            });
        }
        self.current.failure = result.failure.filter(|_| next == Status::Unhealthy);
    }

    fn event(&self, name: &str) -> Option<StatusEvent> {
        let status = match self.current.status {
            Status::Healthy if self.consecutive_failures == 0 => EventStatus::Healthy,
            Status::Unhealthy => EventStatus::Unhealthy,
            Status::Checking | Status::NoData | Status::Healthy => return None,
        };
        Some(StatusEvent {
            service_name: name.to_string(),
            status,
            occurred_at: self.current.status_since?,
            failure: self.current.failure,
        })
    }
}

struct Service {
    name: String,
    label: ServiceText,
    impact: ServiceText,
    endpoint: Option<Url>,
    target: Option<String>,
    state: std::sync::Mutex<ProbeState>,
    last_manual: std::sync::Mutex<Option<Instant>>,
    in_flight: Mutex<()>,
    /// Status changes whose database write failed, oldest first. Retried on the next check.
    outbox: std::sync::Mutex<Vec<StatusEvent>>,
}

impl Service {
    fn new(
        name: String,
        label: ServiceText,
        impact: ServiceText,
        endpoint: Option<Url>,
        target: Option<String>,
    ) -> Self {
        Self {
            name,
            label,
            impact,
            endpoint,
            target,
            state: std::sync::Mutex::default(),
            last_manual: std::sync::Mutex::default(),
            in_flight: Mutex::default(),
            outbox: std::sync::Mutex::default(),
        }
    }

    fn view(&self) -> ServiceView {
        ServiceView {
            name: self.name.clone(),
            label: self.label.clone(),
            impact: self.impact.clone(),
            target: self.target.clone(),
            current: lock(&self.state).current.clone(),
            recent_events: vec![],
            availability: None,
        }
    }
}

pub(crate) struct Monitor {
    http: Client,
    services: Vec<Service>,
    /// 即時語音節點（S-19）：隨登錄表增減，由 `sync_voice_nodes` 維護。
    voice: std::sync::Mutex<Vec<std::sync::Arc<Service>>>,
    metrics: std::sync::Arc<crate::metrics::Metrics>,
}

impl Monitor {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(config: &[MonitoredService]) -> Self {
        Self::with_metrics(config, std::sync::Arc::default())
    }

    pub(crate) fn with_metrics(
        config: &[MonitoredService],
        metrics: std::sync::Arc<crate::metrics::Metrics>,
    ) -> Self {
        let database = Service::new(
            DATABASE.into(),
            ServiceText::Text("PostgreSQL".into()),
            ServiceText::Translations(BTreeMap::from([
                ("zh-TW".into(), "全站".into()),
                ("en".into(), "Entire site".into()),
                ("es".into(), "Todo el sitio".into()),
            ])),
            None,
            None,
        );
        let services = std::iter::once(database)
            .chain(config.iter().map(|item| {
                Service::new(
                    item.name.clone(),
                    item.label.clone(),
                    item.impact.clone(),
                    Some(item.endpoint.clone()),
                    Some(item.target()),
                )
            }))
            .collect();
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .timeout(READY_DB_TIMEOUT)
            .build()
            .expect("failed to construct service probe HTTP client");
        Self {
            http,
            services,
            voice: std::sync::Mutex::default(),
            metrics,
        }
    }

    async fn probe(&self, service: &Service, pool: &PgPool) -> ProbeResult {
        crate::telemetry::in_component_span_with_target(
            "service_probe",
            "health_check",
            Some(&service.name),
            async {
                let start = Instant::now();
                let elapsed_ms = || u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
                let result = if let Some(url) = &service.endpoint {
                    let mut req = self.http.get(url.clone());
                    // 只透傳 W3C traceparent（不帶 cookie 或授權標頭，S-09.3）
                    if let Some(tp) = crate::telemetry::current_traceparent() {
                        req = req.header(crate::telemetry::TRACEPARENT_HEADER, tp);
                    }
                    match req.send().await {
                        Ok(response) => ProbeResult {
                            failure: (!response.status().is_success())
                                .then_some(ServiceFailure::Non2xx),
                            response_time_ms: Some(elapsed_ms()),
                        },
                        Err(error) if error.is_timeout() => ProbeResult {
                            failure: Some(ServiceFailure::Timeout),
                            response_time_ms: None,
                        },
                        Err(_) => ProbeResult {
                            failure: Some(ServiceFailure::ConnectionFailed),
                            response_time_ms: None,
                        },
                    }
                } else {
                    match crate::probe_database(pool).await {
                        Ok(Ok(_)) => ProbeResult {
                            failure: None,
                            response_time_ms: Some(elapsed_ms()),
                        },
                        Err(_) | Ok(Err(sqlx::Error::PoolTimedOut)) => ProbeResult {
                            failure: Some(ServiceFailure::Timeout),
                            response_time_ms: None,
                        },
                        Ok(Err(_)) => ProbeResult {
                            failure: Some(ServiceFailure::ConnectionFailed),
                            response_time_ms: None,
                        },
                    }
                };
                let outcome = match result.failure {
                    None => "ok",
                    Some(ServiceFailure::ConnectionFailed) => "connection_failed",
                    Some(ServiceFailure::Timeout) => "timeout",
                    Some(ServiceFailure::Non2xx) => "non_2xx",
                };
                self.metrics
                    .record_service_probe(&service.name, outcome, start.elapsed());
                result
            },
        )
        .await
    }

    // The caller serializes checks for this service, including event persistence.
    /// 回傳這次探測是否成功（無 failure），供排程輪次決定背景工作指標的結果標籤。
    async fn check(&self, service: &Service, pool: &PgPool, trigger: Trigger) -> bool {
        let result = self.probe(service, pool).await;
        let ok = result.failure.is_none();
        let pending = {
            let mut state = lock(&service.state);
            state.observe(result, Utc::now(), trigger);
            let mut outbox = lock(&service.outbox);
            if let Some(event) = state.event(&service.name)
                && outbox.last().is_none_or(|last| last.status != event.status)
            {
                if outbox.len() >= MAX_PENDING_EVENTS {
                    outbox.remove(0);
                }
                outbox.push(event);
            }
            outbox.clone()
        };
        let mut written = 0;
        for event in pending {
            match tokio::time::timeout(HISTORY_TIMEOUT, record_event(pool, event)).await {
                Ok(Ok(())) => written += 1,
                Ok(Err(error)) => {
                    tracing::warn!(
                        service = %service.name,
                        class = %crate::error::db_error_class(&error),
                        "service status event write failed"
                    );
                    break;
                }
                Err(_) => {
                    tracing::warn!(
                        service = %service.name,
                        "service status event write timed out"
                    );
                    break;
                }
            }
        }
        lock(&service.outbox).drain(..written);
        ok
    }

    /// 周邊服務（不含資料庫）的狀態摘要，給首頁健康度使用。
    pub(crate) fn summary(&self) -> ServiceSummary {
        let mut summary = ServiceSummary::default();
        for service in self.services.iter().filter(|s| s.name != DATABASE) {
            summary.total += 1;
            match lock(&service.state).current.status {
                Status::Healthy => summary.healthy += 1,
                Status::Unhealthy => summary.unhealthy += 1,
                Status::Checking | Status::NoData => summary.unknown += 1,
            }
        }
        summary
    }

    /// 各監控服務與語音節點目前的健康狀態（供 Prometheus `socrates_service_up` gauge 使用）。
    pub(crate) fn statuses(&self) -> Vec<(String, bool)> {
        let voice = self.voice_services();
        self.services
            .iter()
            .chain(voice.iter().map(|service| &**service))
            .map(|service| {
                (
                    service.name.clone(),
                    lock(&service.state).current.status == Status::Healthy,
                )
            })
            .collect()
    }

    async fn view(&self, pool: &PgPool, period_days: i64) -> ServicesView {
        let services: Vec<_> = self.services.iter().collect();
        let (services, history_available) = rows(&services, pool, period_days).await;
        ServicesView {
            services,
            history_available,
            period_days,
            voice_nodes: Vec::new(),
        }
    }

    /// 讓語音節點的監控項目與登錄表一致：新增的加入、消失的移除、網址或名稱變了就重建。
    pub(crate) fn sync_voice_nodes(&self, nodes: &[crate::voice_nodes::Node]) {
        let mut voice = lock(&self.voice);
        let wanted: Vec<_> = nodes
            .iter()
            .filter_map(|node| {
                let endpoint = crate::voice_nodes::health_url(&node.realtime_url)?;
                Some((voice_service_name(node.id), node.name.clone(), endpoint))
            })
            .collect();
        voice.retain(|service| {
            wanted.iter().any(|(name, label, endpoint)| {
                *name == service.name
                    && service.endpoint.as_ref() == Some(endpoint)
                    && matches!(&service.label, ServiceText::Text(text) if text == label)
            })
        });
        for (name, label, endpoint) in wanted {
            if voice.iter().all(|service| service.name != name) {
                let target = endpoint_target(&endpoint);
                voice.push(std::sync::Arc::new(Service::new(
                    name,
                    ServiceText::Text(label),
                    ServiceText::Translations(BTreeMap::from([
                        ("zh-TW".into(), "即時語音".into()),
                        ("en".into(), "Real-time voice".into()),
                        ("es".into(), "Voz en tiempo real".into()),
                    ])),
                    Some(endpoint),
                    Some(target),
                )));
            }
        }
    }

    fn voice_services(&self) -> Vec<std::sync::Arc<Service>> {
        lock(&self.voice).clone()
    }

    /// 各語音節點的監控資料，鍵為 `voice_service_name`。
    async fn voice_views(
        &self,
        pool: &PgPool,
        period_days: i64,
    ) -> std::collections::HashMap<String, ServiceView> {
        let owned = self.voice_services();
        let services: Vec<_> = owned.iter().map(|service| &**service).collect();
        let (views, _) = rows(&services, pool, period_days).await;
        views
            .into_iter()
            .map(|view| (view.name.clone(), view))
            .collect()
    }
}

pub(crate) fn voice_service_name(id: crate::voice_nodes::NodeId) -> String {
    format!("voice-node:{id}")
}

fn endpoint_target(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}{}", url.path()),
        None => format!("{host}{}", url.path()),
    }
}

/// 各服務的目前狀態加上可用率與最近的狀態變化；回傳值的第二項表示歷史是否讀得到。
async fn rows(services: &[&Service], pool: &PgPool, period_days: i64) -> (Vec<ServiceView>, bool) {
    let now = Utc::now();
    let names: Vec<_> = services
        .iter()
        .map(|service| service.name.as_str())
        .collect();
    let history = tokio::time::timeout(HISTORY_TIMEOUT, load_history(pool, &names, now)).await;
    let mut history = match history {
        Ok(Ok(events)) => Some(events),
        Ok(Err(error)) => {
            tracing::warn!(class = %crate::error::db_error_class(&error), "service status history read failed");
            None
        }
        Err(_) => {
            tracing::warn!("service status history read timed out");
            None
        }
    };
    let mut views: Vec<_> = services
        .iter()
        .map(|service| {
            let mut row = service.view();
            if let Some(events) = history
                .as_mut()
                .and_then(|history| history.remove(&service.name))
            {
                row.availability = availability(&events, now - TimeDelta::days(period_days), now);
                row.recent_events = events.into_iter().rev().take(RECENT_EVENTS).collect();
            }
            row
        })
        .collect();
    views.sort_by_key(|service| service.current.status != Status::Unhealthy);
    (views, history.is_some())
}

/// 解析 `SERVICE_PROBE_INTERVAL_SECS`：未設定或空白用預設 30 秒；
/// 不是整數或不在 10 至 300 秒內時回錯誤，讓啟動失敗而不是悄悄改用別的值。
pub fn parse_probe_interval(raw: Option<&str>) -> Result<Duration, ConfigError> {
    let Some(raw) = raw.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return Ok(DEFAULT_PROBE_INTERVAL);
    };
    raw.parse::<u64>()
        .ok()
        .filter(|secs| (MIN_PROBE_INTERVAL_SECS..=MAX_PROBE_INTERVAL_SECS).contains(secs))
        .map(Duration::from_secs)
        .ok_or_else(|| {
            ConfigError(format!(
                "SERVICE_PROBE_INTERVAL_SECS must be a whole number of seconds between \
                 {MIN_PROBE_INTERVAL_SECS} and {MAX_PROBE_INTERVAL_SECS}"
            ))
        })
}

async fn run_scheduled_cycle(state: &AppState) {
    crate::telemetry::in_component_span("background_job", "service_monitor_cycle", async {
        let job_metric = state.metrics.track_background_job("service_monitor");
        let monitor = &state.service_monitor;
        monitor.sync_voice_nodes(&state.voice_nodes.all(state).await);
        let voice = monitor.voice_services();
        let all = monitor
            .services
            .iter()
            .chain(voice.iter().map(|service| &**service));
        let results = join_all(all.map(|service| async {
            if let Ok(_guard) = service.in_flight.try_lock() {
                monitor
                    .check(service, &state.pool, Trigger::Scheduled)
                    .await
            } else {
                true
            }
        }))
        .await;
        let all_ok = results.into_iter().all(std::convert::identity);
        job_metric.finish(if all_ok { "ok" } else { "degraded" });
    })
    .await;
}

pub fn spawn(state: &AppState) {
    let state = state.clone();
    let probe_interval = state.config.service_probe_interval;
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(probe_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            run_scheduled_cycle(&state).await;
        }
    });
}

async fn record_event(pool: &PgPool, event: StatusEvent) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    // Serialize the read/insert across processes. The text prefix namespaces the advisory lock.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('service-status:' || $1, 0))")
        .bind(&event.service_name)
        .execute(&mut *tx)
        .await?;
    let latest: Option<(EventStatus, DateTime<Utc>)> = sqlx::query_as(
        "SELECT status, occurred_at FROM service_status_events
         WHERE service_name = $1
         ORDER BY occurred_at DESC, id DESC LIMIT 1",
    )
    .bind(&event.service_name)
    .fetch_optional(&mut *tx)
    .await?;
    if latest.is_none_or(|(status, at)| status != event.status && at <= event.occurred_at) {
        sqlx::query("INSERT INTO service_status_events (service_name, status, occurred_at, failure) VALUES ($1, $2, $3, $4)")
            .bind(event.service_name).bind(event.status).bind(event.occurred_at).bind(event.failure)
            .execute(&mut *tx).await?;
    }
    tx.commit().await
}

async fn load_history(
    pool: &PgPool,
    names: &[&str],
    now: DateTime<Utc>,
) -> Result<BTreeMap<String, Vec<StatusEvent>>, sqlx::Error> {
    let rows: Vec<StatusEvent> = sqlx::query_as(
        "SELECT service_name, status, occurred_at, failure FROM service_status_events
         WHERE service_name = ANY($1) AND occurred_at >= $2 AND occurred_at <= $3
         ORDER BY occurred_at, id",
    )
    .bind(names)
    .bind(now - TimeDelta::days(RETENTION_DAYS))
    .bind(now)
    .fetch_all(pool)
    .await?;
    let mut by_service = BTreeMap::<_, Vec<_>>::new();
    for row in rows {
        by_service
            .entry(row.service_name.clone())
            .or_default()
            .push(row);
    }
    Ok(by_service)
}

fn availability(events: &[StatusEvent], from: DateTime<Utc>, to: DateTime<Utc>) -> Option<f64> {
    let mut known = 0;
    let mut healthy = 0;
    for (index, event) in events.iter().enumerate() {
        let end = events.get(index + 1).map_or(to, |next| next.occurred_at);
        let duration = (end - event.occurred_at.max(from))
            .num_microseconds()
            .unwrap_or(0)
            .max(0);
        known += duration;
        if event.status == EventStatus::Healthy {
            healthy += duration;
        }
    }
    (known > 0).then(|| healthy as f64 / known as f64)
}

#[derive(Serialize, ToSchema)]
pub(crate) struct ServiceView {
    name: String,
    label: ServiceText,
    impact: ServiceText,
    target: Option<String>,
    #[serde(flatten)]
    current: CurrentStatus,
    /// Healthy time divided by known time (0–1), null when no known interval exists.
    availability: Option<f64>,
    recent_events: Vec<StatusEvent>,
}

/// 周邊服務的狀態摘要（不含資料庫）。
#[derive(Default, Serialize, ToSchema)]
pub(crate) struct ServiceSummary {
    pub total: u32,
    pub healthy: u32,
    pub unhealthy: u32,
    /// 檢查中或尚無資料
    pub unknown: u32,
}

/// 即時語音節點（S-19）：監控資料與周邊服務相同（狀態、回應時間、可用率、歷史），另加節點自己的欄位。
#[derive(Serialize, ToSchema)]
pub(crate) struct ServiceVoiceNode {
    #[serde(flatten)]
    service: ServiceView,
    /// `active`、`draining` 或 `disabled`
    node_status: String,
    /// 是否接新場次的路由判斷（連續 3 次探測失敗為 false），與 `status` 的判斷規則不同
    healthy: bool,
    sessions: usize,
    /// 環境節點預設不設上限
    max_sessions: Option<usize>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct ServicesView {
    services: Vec<ServiceView>,
    history_available: bool,
    period_days: i64,
    voice_nodes: Vec<ServiceVoiceNode>,
}

#[derive(Deserialize)]
pub(crate) struct ServicesQuery {
    period_days: Option<i64>,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct CheckBody {
    name: String,
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/services", get(list))
        .route("/api/admin/services/check", post(check))
        .route_layer(axum::middleware::map_response(
            |mut response: axum::response::Response| async move {
                response
                    .headers_mut()
                    .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
                response
            },
        ))
}

#[utoipa::path(get, path = "/api/admin/services", tag = "admin",
    params(("period_days" = Option<i64>, Query, description = "1–30 days, defaults to 30")),
    responses((status = 200, body = ServicesView), (status = 400, body = ErrorBody),
              (status = 401, body = ErrorBody), (status = 403, body = ErrorBody)))]
pub(crate) async fn list(
    _: RequireAdmin,
    State(state): State<AppState>,
    query: Result<Query<ServicesQuery>, QueryRejection>,
) -> Result<Json<ServicesView>, ApiError> {
    let period_days = query
        .ok()
        .map(|Query(query)| query.period_days.unwrap_or(RETENTION_DAYS))
        .filter(|days| (1..=RETENTION_DAYS).contains(days))
        .ok_or_else(|| ApiError::bad_request("invalid_service_period", "期間須介於 1 至 30 天"))?;
    let monitor = &state.service_monitor;
    let nodes = state.voice_nodes.all(&state).await;
    monitor.sync_voice_nodes(&nodes);
    let (mut view, mut voice) = tokio::join!(
        monitor.view(&state.pool, period_days),
        monitor.voice_views(&state.pool, period_days)
    );
    // 部署預設的 MONITORED_SERVICES 已有檢查 cascade /health 的項目；語音節點探測同一個位址時，
    // 只留節點那一列（資訊較完整），避免同一個目標列兩次。
    let voice_targets: Vec<_> = voice.values().filter_map(|v| v.target.clone()).collect();
    view.services
        .retain(|s| s.target.as_ref().is_none_or(|t| !voice_targets.contains(t)));
    let counts = state.voice_sessions.counts();
    view.voice_nodes = nodes
        .into_iter()
        .filter_map(|node| {
            Some(ServiceVoiceNode {
                service: voice.remove(&voice_service_name(node.id))?,
                healthy: state.voice_nodes.is_healthy(node.id),
                sessions: counts.get(&node.id).copied().unwrap_or(0),
                node_status: node.status.as_str().into(),
                max_sessions: node.max_sessions,
            })
        })
        .collect();
    Ok(Json(view))
}

#[utoipa::path(post, path = "/api/admin/services/check", tag = "admin", request_body = CheckBody,
    responses((status = 200, body = ServiceView), (status = 401, body = ErrorBody),
              (status = 403, body = ErrorBody), (status = 404, body = ErrorBody),
              (status = 429, body = ErrorBody)))]
pub(crate) async fn check(
    _: RequireAdmin,
    State(state): State<AppState>,
    Json(body): Json<CheckBody>,
) -> Result<Json<ServiceView>, ApiError> {
    let monitor = &state.service_monitor;
    monitor.sync_voice_nodes(&state.voice_nodes.all(&state).await);
    let voice = monitor.voice_services();
    let service = monitor
        .services
        .iter()
        .chain(voice.iter().map(|service| &**service))
        .find(|service| service.name == body.name)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "service_not_found", "找不到服務"))?;
    {
        let mut last = lock(&service.last_manual);
        if last.is_some_and(|at| at.elapsed() < MANUAL_INTERVAL) {
            return Err(ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "service_check_rate_limited",
                "檢查過於頻繁",
            ));
        }
        *last = Some(Instant::now());
    }
    if let Ok(_guard) = service.in_flight.try_lock() {
        monitor.check(service, &state.pool, Trigger::Manual).await;
    } else {
        // An in-flight scheduled check already supplies the fresh result.
        let _guard = service.in_flight.lock().await;
    }
    Ok(Json(service.view()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Response};
    use futures_util::stream;
    use std::{
        convert::Infallible,
        future::pending,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    #[sqlx::test]
    async fn probes_record_only_transitions_and_keep_the_running_state_if_storage_fails(
        pool: PgPool,
    ) {
        let calls = Arc::new(AtomicUsize::new(0));
        let handler = {
            let calls = calls.clone();
            move || async move {
                match calls.fetch_add(1, Ordering::Relaxed) {
                    1 | 2 => pending().await,
                    3 => Response::builder()
                        .status(StatusCode::TEMPORARY_REDIRECT)
                        .header(header::LOCATION, "/must-not-follow")
                        .header(header::CONTENT_LENGTH, "100000000")
                        .body(Body::from_stream(stream::pending::<
                            Result<String, Infallible>,
                        >()))
                        .unwrap(),
                    _ => Response::new(Body::empty()),
                }
            }
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = listener.local_addr().unwrap();
        let redirects = Arc::new(AtomicUsize::new(0));
        let redirected = redirects.clone();
        let server = tokio::spawn(async move {
            let app = Router::new().route("/health", get(handler)).route(
                "/must-not-follow",
                get(move || {
                    redirected.fetch_add(1, Ordering::Relaxed);
                    async { StatusCode::OK }
                }),
            );
            axum::serve(listener, app).await.unwrap();
        });
        let config = MonitoredService::parse(&format!(
            r#"[{{"name":"asr","label":"ASR","impact":"Voice","url":"http://{endpoint}/health"}}]"#
        ))
        .unwrap();
        let monitor = Monitor::new(&config);
        let service = &monitor.services[1];
        let results = [
            (Status::Healthy, None, 1),
            (Status::Healthy, None, 1),
            (Status::Unhealthy, Some(ServiceFailure::Timeout), 2),
            (Status::Unhealthy, Some(ServiceFailure::Non2xx), 2),
            (Status::Healthy, None, 3),
            (Status::Healthy, None, 3),
        ];
        for (index, (status, failure, expected_count)) in results.into_iter().enumerate() {
            let start = Instant::now();
            monitor.check(service, &pool, Trigger::Scheduled).await;
            let current = service.view().current;
            assert_eq!((current.status, current.failure), (status, failure));
            let events: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM service_status_events WHERE service_name = 'asr'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(events, expected_count);
            if index == 1 || index == 2 {
                assert!(start.elapsed() >= READY_DB_TIMEOUT);
            }
        }
        let other_monitor = Monitor::new(&config);
        other_monitor
            .check(&other_monitor.services[1], &pool, Trigger::Scheduled)
            .await;
        let history = load_history(&pool, &["asr"], Utc::now()).await.unwrap();
        assert_eq!(history["asr"].len(), 3);
        assert_eq!(history["asr"][1].failure, Some(ServiceFailure::Timeout));
        let closed = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://127.0.0.1:1/unused")
            .unwrap();
        closed.close().await;
        monitor.check(service, &closed, Trigger::Scheduled).await;
        let view = monitor.view(&closed, 30).await;
        assert!(!view.history_available);
        assert_eq!(view.services[1].current.status, Status::Healthy);
        assert_eq!(calls.load(Ordering::Relaxed), 8);
        assert_eq!(redirects.load(Ordering::Relaxed), 0);
        server.abort();
    }

    #[sqlx::test]
    async fn concurrent_writers_for_the_same_service_insert_one_event(pool: PgPool) {
        let event = StatusEvent {
            service_name: "asr".to_string(),
            status: EventStatus::Healthy,
            occurred_at: Utc::now(),
            failure: None,
        };
        tokio::try_join!(
            record_event(&pool, event.clone()),
            record_event(&pool, event)
        )
        .unwrap();
        let rows = load_history(&pool, &["asr"], Utc::now()).await.unwrap();
        assert_eq!(rows["asr"].len(), 1);
    }

    #[test]
    fn availability_counts_only_the_known_part_of_the_window() {
        let now = Utc::now();
        let event = |status, hours| StatusEvent {
            service_name: "asr".to_string(),
            status,
            occurred_at: now - TimeDelta::hours(hours),
            failure: None,
        };
        let events = [
            event(EventStatus::Healthy, 3),
            event(EventStatus::Unhealthy, 1),
        ];
        assert_eq!(
            availability(&events, now - TimeDelta::hours(4), now),
            Some(2.0 / 3.0)
        );
        assert_eq!(
            availability(&events, now - TimeDelta::hours(2), now),
            Some(0.5)
        );
        assert_eq!(availability(&[], now - TimeDelta::hours(4), now), None);
    }

    fn failed() -> ProbeResult {
        ProbeResult {
            failure: Some(ServiceFailure::Timeout),
            response_time_ms: None,
        }
    }

    #[test]
    fn unhealthy_state_starts_at_the_first_failed_probe() {
        let t0 = Utc::now();
        let mut state = ProbeState::default();
        state.observe(failed(), t0, Trigger::Scheduled);
        state.observe(failed(), t0 + TimeDelta::seconds(30), Trigger::Scheduled);
        assert_eq!(state.current.status, Status::Unhealthy);
        assert_eq!(state.current.status_since, Some(t0));
        assert_eq!(state.event("asr").unwrap().occurred_at, t0);
    }

    #[test]
    fn manual_failures_do_not_count_toward_the_consecutive_failure_rule() {
        let t0 = Utc::now();
        let mut state = ProbeState::default();
        state.observe(failed(), t0, Trigger::Scheduled);
        state.observe(failed(), t0 + TimeDelta::seconds(10), Trigger::Manual);
        state.observe(failed(), t0 + TimeDelta::seconds(20), Trigger::Manual);
        assert_eq!(state.current.status, Status::NoData);
        state.observe(failed(), t0 + TimeDelta::seconds(30), Trigger::Scheduled);
        assert_eq!(state.current.status, Status::Unhealthy);
    }

    #[test]
    fn probe_interval_defaults_to_30_seconds_when_unset_or_blank() {
        for raw in [None, Some(""), Some("  \n")] {
            assert_eq!(parse_probe_interval(raw).unwrap(), Duration::from_secs(30));
        }
    }

    #[test]
    fn probe_interval_accepts_the_whole_10_to_300_second_range() {
        assert_eq!(
            parse_probe_interval(Some("10")).unwrap(),
            Duration::from_secs(10)
        );
        assert_eq!(
            parse_probe_interval(Some(" 60 ")).unwrap(),
            Duration::from_secs(60)
        );
        assert_eq!(
            parse_probe_interval(Some("300")).unwrap(),
            Duration::from_secs(300)
        );
    }

    #[test]
    fn probe_interval_outside_the_range_or_not_a_whole_number_fails_startup() {
        for raw in ["0", "9", "301", "3600", "-30", "30.5", "30s", "abc"] {
            let error = parse_probe_interval(Some(raw)).unwrap_err().to_string();
            assert!(
                error.contains("SERVICE_PROBE_INTERVAL_SECS"),
                "{raw}: {error}"
            );
            assert!(
                error.contains("10") && error.contains("300"),
                "{raw}: {error}"
            );
        }
    }

    #[test]
    fn blank_monitored_services_means_no_services() {
        assert!(MonitoredService::parse("").unwrap().is_empty());
        assert!(MonitoredService::parse("  \n").unwrap().is_empty());
        assert!(MonitoredService::parse("{}").is_err());
    }

    #[sqlx::test]
    async fn database_outage_is_recorded_once_the_database_returns(pool: PgPool) {
        let monitor = Monitor::new(&[]);
        let service = &monitor.services[0];
        monitor.check(service, &pool, Trigger::Scheduled).await;
        let down = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(200))
            .connect_lazy("postgres://x:x@127.0.0.1:1/x")
            .unwrap();
        for _ in 0..2 {
            monitor.check(service, &down, Trigger::Scheduled).await;
        }
        assert_eq!(service.view().current.status, Status::Unhealthy);
        monitor.check(service, &pool, Trigger::Scheduled).await;
        monitor.check(service, &pool, Trigger::Scheduled).await;
        let statuses: Vec<String> = sqlx::query_scalar(
            "SELECT status FROM service_status_events WHERE service_name = 'database' ORDER BY occurred_at, id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(statuses, ["healthy", "unhealthy", "healthy"]);
        assert!(lock(&service.outbox).is_empty());
    }

    #[tokio::test]
    async fn scheduled_cycle_shares_trace_across_probes_and_records_background_job_metrics() {
        use std::{io, sync::Mutex as StdMutex};

        use axum::http::HeaderMap;
        use serde_json::Value;
        use tracing_subscriber::fmt::MakeWriter;

        use crate::{
            ai::{AiError, AiProvider, AiRequest, AiStream},
            config::Config,
            identity::{AuthRequest, Identity, IdentityError, IdentityProvider},
            key_pool::KeyPool,
        };

        #[derive(Clone, Default)]
        struct Captured(Arc<StdMutex<Vec<u8>>>);
        struct Sink(Arc<StdMutex<Vec<u8>>>);
        impl io::Write for Sink {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        impl<'a> MakeWriter<'a> for Captured {
            type Writer = Sink;
            fn make_writer(&'a self) -> Sink {
                Sink(self.0.clone())
            }
        }

        struct DummyId;
        #[async_trait::async_trait]
        impl IdentityProvider for DummyId {
            fn start(&self) -> AuthRequest {
                AuthRequest {
                    url: String::new(),
                    state: String::new(),
                    nonce: String::new(),
                    pkce_verifier: String::new(),
                }
            }
            async fn finish(&self, _: &str, _: &str, _: &str) -> Result<Identity, IdentityError> {
                Err(IdentityError)
            }
        }

        struct DummyAi;
        #[async_trait::async_trait]
        impl AiProvider for DummyAi {
            async fn stream_chat(&self, _: AiRequest) -> Result<AiStream, AiError> {
                Err(AiError::unclassified())
            }
        }

        let captured = Captured::default();
        let subscriber = crate::logging::json_subscriber()
            .with_env_filter(crate::logging::env_filter(Some("info")))
            .with_writer(captured.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let received = Arc::new(StdMutex::new(BTreeMap::<String, String>::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = listener.local_addr().unwrap();
        let server = {
            let received = Arc::clone(&received);
            tokio::spawn(async move {
                let record =
                    |name: &'static str, received: Arc<StdMutex<BTreeMap<String, String>>>| {
                        move |headers: HeaderMap| async move {
                            if let Some(tp) = headers
                                .get(crate::telemetry::TRACEPARENT_HEADER)
                                .and_then(|v| v.to_str().ok())
                            {
                                received
                                    .lock()
                                    .unwrap()
                                    .insert(name.to_string(), tp.to_string());
                            }
                            StatusCode::OK
                        }
                    };
                let app = Router::new()
                    .route("/a", get(record("svc-a", Arc::clone(&received))))
                    .route("/b", get(record("svc-b", received)));
                axum::serve(listener, app).await.unwrap();
            })
        };

        let monitored_services = MonitoredService::parse(&format!(
            r#"[
                {{"name":"svc-a","label":"A","impact":"Voice","url":"http://{endpoint}/a"}},
                {{"name":"svc-b","label":"B","impact":"Voice","url":"http://{endpoint}/b"}}
            ]"#
        ))
        .unwrap();

        let closed_pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://unused@127.0.0.1:1/unused")
            .unwrap();
        closed_pool.close().await;

        let config = Config {
            database_url: String::new(),
            app_base_url: "https://app.test".into(),
            google_client_id: "c".into(),
            google_client_secret: "s".into(),
            google_redirect_url: "https://app.test/cb".into(),
            google_translation_api_key: None,
            google_books_api_key: None,
            admin_emails: vec![],
            admin_initial_password: None,
            cookie_secure: true,
            listen_addr: "127.0.0.1:0".into(),
            trusted_proxies: vec![],
            login_rate_burst: 10,
            login_rate_replenish: Duration::from_secs(1),
            ai_usage_max: 10,
            ai_usage_window: Duration::from_secs(60),
            frontend_dir: None,
            openai_keys: Arc::new(KeyPool::empty()),
            openai_model: "m".into(),
            openai_fallback_models: vec![],
            openai_reasoning_effort: None,
            openai_base_url: "http://127.0.0.1:1".into(),
            openai_stt_base_url: None,
            openai_tts_base_url: None,
            openai_stt_model: None,
            openai_tts_model: None,
            openai_tts_voice: "alloy".into(),
            cascade_realtime_url: None,
            cascade_llm_token: None,
            cascade_max_sessions: None,
            internal_listen_addr: None,
            wrap_up_turn: 3,
            max_turns: 5,
            ai_first_token_timeout: Duration::from_millis(50),
            ai_total_timeout: Duration::from_millis(50),
            argument_tree_timeout: Duration::from_millis(50),
            request_timeout: Duration::from_secs(5),
            demo_mode: false,
            smtp: None,
            teacher_key_cipher: None,
            classroom_ai_routing: false,
            monitored_services,
            service_probe_interval: Duration::from_secs(30),
            database_max_connections: 1,
            metrics_token: None,
        };
        let state = AppState::new(closed_pool, config, Arc::new(DummyId), Arc::new(DummyAi));

        // 第一輪：含失效的 database 探測 → outcome="degraded"
        run_scheduled_cycle(&state).await;

        // 1. 同一輪次內的所有 HTTP 探測共用同一個 trace_id，且各自擁有不同的子 span_id
        let cycle_trace_id = {
            let headers = received.lock().unwrap();
            let tp_a: Vec<&str> = headers["svc-a"].split('-').collect();
            let tp_b: Vec<&str> = headers["svc-b"].split('-').collect();
            assert_eq!(tp_a.len(), 4);
            assert_eq!(tp_b.len(), 4);
            assert_eq!(
                tp_a[1], tp_b[1],
                "probes in the same scheduled cycle must share one trace_id"
            );
            assert_ne!(
                tp_a[2], tp_b[2],
                "each service probe must have its own child span_id"
            );
            tp_a[1].to_owned()
        };

        // 2. 輪次內產生的警告日誌歸屬在 background_job / service_monitor_cycle span 下並帶相同 trace_id 與 service 名稱
        let out = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        let warn_entry: Value = out
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .find(|e| e["fields"]["message"] == "service status event write failed")
            .expect("expected DB write warning inside cycle");
        assert_eq!(warn_entry["span"]["component"], "background_job");
        assert_eq!(warn_entry["span"]["operation"], "service_monitor_cycle");
        assert_eq!(warn_entry["span"]["trace_id"], cycle_trace_id);
        assert!(warn_entry["fields"]["service"].is_string());

        // 3. 第二輪：全部服務健康時記錄 outcome="ok" 且產生全新的 cycle trace_id
        let mut ok_monitor =
            Monitor::with_metrics(&state.config.monitored_services, state.metrics.clone());
        ok_monitor.services.retain(|s| s.name != DATABASE);
        let mut ok_state = state.clone();
        ok_state.service_monitor = Arc::new(ok_monitor);
        run_scheduled_cycle(&ok_state).await;
        server.abort();

        let headers_after = received.lock().unwrap();
        let tp_a_2: Vec<&str> = headers_after["svc-a"].split('-').collect();
        assert_ne!(
            tp_a_2[1], cycle_trace_id,
            "each scheduled cycle must start a fresh trace_id"
        );

        // 4. 背景工作（degraded 與 ok 兩條分支）與個別服務探測指標皆正確記錄
        let prom = state.metrics.render(&state);
        assert!(prom.contains(
            "socrates_background_jobs_total{job=\"service_monitor\",outcome=\"degraded\"} 1"
        ));
        assert!(
            prom.contains(
                "socrates_background_jobs_total{job=\"service_monitor\",outcome=\"ok\"} 1"
            )
        );
        assert!(prom.contains("socrates_background_jobs_in_flight{job=\"service_monitor\"} 0"));
        assert!(
            prom.contains(
                "socrates_background_job_duration_seconds_count{job=\"service_monitor\"} 2"
            )
        );
        assert!(prom.contains("socrates_service_probes_total{service=\"svc-a\",outcome=\"ok\"} 2"));
        assert!(prom.contains("socrates_service_probes_total{service=\"svc-b\",outcome=\"ok\"} 2"));
        assert!(prom.contains(
            "socrates_service_probes_total{service=\"database\",outcome=\"connection_failed\"} 1"
        ));
    }
}
