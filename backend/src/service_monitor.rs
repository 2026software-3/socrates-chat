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
const PROBE_INTERVAL: Duration = Duration::from_secs(30);
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
enum Failure {
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
    failure: Option<Failure>,
}

#[derive(Clone, Default, Serialize, ToSchema)]
struct CurrentStatus {
    status: Status,
    failure: Option<Failure>,
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
    failure: Option<Failure>,
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
}

impl Monitor {
    pub(crate) fn new(config: &[MonitoredService]) -> Self {
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
        Self { http, services }
    }

    async fn probe(&self, service: &Service, pool: &PgPool) -> ProbeResult {
        let start = Instant::now();
        let elapsed = || u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        let failure = if let Some(url) = &service.endpoint {
            match self.http.get(url.clone()).send().await {
                Ok(response) => {
                    return ProbeResult {
                        failure: (!response.status().is_success()).then_some(Failure::Non2xx),
                        response_time_ms: Some(elapsed()),
                    };
                }
                Err(error) if error.is_timeout() => Failure::Timeout,
                Err(_) => Failure::ConnectionFailed,
            }
        } else {
            match crate::probe_database(pool).await {
                Ok(Ok(_)) => {
                    return ProbeResult {
                        failure: None,
                        response_time_ms: Some(elapsed()),
                    };
                }
                Err(_) | Ok(Err(sqlx::Error::PoolTimedOut)) => Failure::Timeout,
                Ok(Err(_)) => Failure::ConnectionFailed,
            }
        };
        ProbeResult {
            failure: Some(failure),
            response_time_ms: None,
        }
    }

    // The caller serializes checks for this service, including event persistence.
    async fn check(&self, service: &Service, pool: &PgPool, trigger: Trigger) {
        let result = self.probe(service, pool).await;
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
                    tracing::warn!(class = %crate::error::db_error_class(&error), "service status event write failed");
                    break;
                }
                Err(_) => {
                    tracing::warn!("service status event write timed out");
                    break;
                }
            }
        }
        lock(&service.outbox).drain(..written);
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

    async fn view(&self, pool: &PgPool, period_days: i64) -> ServicesView {
        let now = Utc::now();
        let names: Vec<_> = self
            .services
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
        let mut services: Vec<_> = self
            .services
            .iter()
            .map(|service| {
                let mut row = service.view();
                if let Some(events) = history
                    .as_mut()
                    .and_then(|history| history.remove(&service.name))
                {
                    row.availability =
                        availability(&events, now - TimeDelta::days(period_days), now);
                    row.recent_events = events.into_iter().rev().take(RECENT_EVENTS).collect();
                }
                row
            })
            .collect();
        services.sort_by_key(|service| service.current.status != Status::Unhealthy);
        ServicesView {
            services,
            history_available: history.is_some(),
            period_days,
        }
    }
}

pub fn spawn(state: &AppState) {
    let monitor = state.service_monitor.clone();
    let pool = state.pool.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(PROBE_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            join_all(monitor.services.iter().map(|service| async {
                if let Ok(_guard) = service.in_flight.try_lock() {
                    monitor.check(service, &pool, Trigger::Scheduled).await;
                }
            }))
            .await;
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

#[derive(Serialize, ToSchema)]
pub(crate) struct ServicesView {
    services: Vec<ServiceView>,
    history_available: bool,
    period_days: i64,
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
    Ok(Json(
        state.service_monitor.view(&state.pool, period_days).await,
    ))
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
    let service = monitor
        .services
        .iter()
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
            (Status::Unhealthy, Some(Failure::Timeout), 2),
            (Status::Unhealthy, Some(Failure::Non2xx), 2),
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
        assert_eq!(history["asr"][1].failure, Some(Failure::Timeout));
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
            failure: Some(Failure::Timeout),
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
}
