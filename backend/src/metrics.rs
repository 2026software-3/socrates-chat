//! 聚合指標登錄與 Prometheus 文字格式匯出（Counter、Gauge、Histogram）。
//!
//! 指標全數放在程序記憶體，以原子操作累加，不引入額外基礎設施（S-09.3），也不含任何
//! 使用者識別、電子郵件、學號、原始 URL 或對話原文（S-02.2）。
//! 透過 `GET /api/admin/metrics` 匯出：支援管理者 session cookie，或由部署環境設定
//! `METRICS_TOKEN` 以 `Authorization: Bearer <token>` 供監控系統抓取。

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    sync::{
        Arc, RwLock,
        atomic::{AtomicI64, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    Router,
    extract::{FromRequestParts, Request, State},
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::{
    AppState,
    auth::RequireAdmin,
    error::{ApiError, ErrorBody},
};

pub const PROMETHEUS_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// 單一指標族允許的最大標籤組合數，防止異常呼叫導致記憶體無限成長。
/// 達上限後，新增的未知標籤組合會彙總至保留的 `<overflow>` 序列，確保總量與告警訊號不遺失。
const MAX_SERIES: usize = 512;
const OVERFLOW_LABEL: &str = "<overflow>";

fn overflow_key(len: usize) -> Vec<String> {
    vec![OVERFLOW_LABEL.to_owned(); len]
}

fn bounded_key<V>(
    map: &BTreeMap<Vec<String>, V>,
    key: Vec<String>,
    label_count: usize,
) -> Vec<String> {
    let overflow = overflow_key(label_count);
    let has_overflow = map.contains_key(&overflow);
    let normal_cap = MAX_SERIES.saturating_sub(1);
    let normal_count = map.len() - usize::from(has_overflow);
    if normal_count >= normal_cap {
        overflow
    } else {
        key
    }
}

const HTTP_DURATION_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
];
const AI_DURATION_BUCKETS: &[f64] = &[0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 20.0, 30.0, 60.0, 120.0];
const JOB_DURATION_BUCKETS: &[f64] =
    &[0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 300.0];
const PROBE_DURATION_BUCKETS: &[f64] = &[0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0];

fn read_lock<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|e| e.into_inner())
}

fn write_lock<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|e| e.into_inner())
}

/// 單調遞增計數器集合（Counter）。
struct CounterVec {
    label_names: &'static [&'static str],
    series: RwLock<BTreeMap<Vec<String>, Arc<AtomicU64>>>,
}

impl CounterVec {
    fn new(label_names: &'static [&'static str]) -> Self {
        Self {
            label_names,
            series: RwLock::new(BTreeMap::new()),
        }
    }

    fn inc(&self, labels: &[&str]) {
        self.inc_by(labels, 1);
    }

    fn inc_by(&self, labels: &[&str], delta: u64) {
        if delta == 0 {
            return;
        }
        debug_assert_eq!(labels.len(), self.label_names.len());
        let key: Vec<String> = labels.iter().map(|s| (*s).to_owned()).collect();
        if let Some(counter) = read_lock(&self.series).get(&key) {
            counter.fetch_add(delta, Ordering::Relaxed);
            return;
        }
        let mut map = write_lock(&self.series);
        if let Some(counter) = map.get(&key) {
            counter.fetch_add(delta, Ordering::Relaxed);
            return;
        }
        let target_key = bounded_key(&map, key, self.label_names.len());
        map.entry(target_key)
            .or_insert_with(|| Arc::new(AtomicU64::new(0)))
            .fetch_add(delta, Ordering::Relaxed);
    }

    fn render(&self, out: &mut String, name: &str, help: &str) {
        let _ = writeln!(out, "# HELP {name} {help}");
        let _ = writeln!(out, "# TYPE {name} counter");
        for (values, counter) in read_lock(&self.series).iter() {
            let v = counter.load(Ordering::Relaxed);
            write_metric_line(out, name, self.label_names, values, v);
        }
    }
}

/// 可增減量表集合（Gauge）。
struct GaugeVec {
    label_names: &'static [&'static str],
    series: RwLock<BTreeMap<Vec<String>, Arc<AtomicI64>>>,
}

impl GaugeVec {
    fn new(label_names: &'static [&'static str]) -> Self {
        Self {
            label_names,
            series: RwLock::new(BTreeMap::new()),
        }
    }

    fn get_or_create(&self, labels: &[&str]) -> Option<Arc<AtomicI64>> {
        debug_assert_eq!(labels.len(), self.label_names.len());
        let key: Vec<String> = labels.iter().map(|s| (*s).to_owned()).collect();
        if let Some(g) = read_lock(&self.series).get(&key) {
            return Some(Arc::clone(g));
        }
        let mut map = write_lock(&self.series);
        if let Some(g) = map.get(&key) {
            return Some(Arc::clone(g));
        }
        let target_key = bounded_key(&map, key, self.label_names.len());
        let g = map
            .entry(target_key)
            .or_insert_with(|| Arc::new(AtomicI64::new(0)));
        Some(Arc::clone(g))
    }

    fn render(&self, out: &mut String, name: &str, help: &str) {
        let _ = writeln!(out, "# HELP {name} {help}");
        let _ = writeln!(out, "# TYPE {name} gauge");
        for (values, gauge) in read_lock(&self.series).iter() {
            let v = gauge.load(Ordering::Relaxed);
            write_metric_line(out, name, self.label_names, values, v);
        }
    }
}

/// 單一數值直方圖（Histogram）；寫入時只對命中的單一桶位做原子加一，匯出時轉為累計分桶。
/// `le="+Inf"` 與 `_count` 共用同一份桶位總和，確保並行寫入時仍嚴格滿足 Prometheus 直方圖不變量。
struct Histogram {
    bounds: &'static [f64],
    buckets: Vec<AtomicU64>,
    sum_micros: AtomicU64,
}

impl Histogram {
    fn new(bounds: &'static [f64]) -> Self {
        let mut buckets = Vec::with_capacity(bounds.len() + 1);
        for _ in 0..=bounds.len() {
            buckets.push(AtomicU64::new(0));
        }
        Self {
            bounds,
            buckets,
            sum_micros: AtomicU64::new(0),
        }
    }

    fn observe(&self, duration: Duration) {
        let secs = duration.as_secs_f64();
        let idx = self
            .bounds
            .iter()
            .position(|&upper| secs <= upper)
            .unwrap_or(self.bounds.len());
        self.buckets[idx].fetch_add(1, Ordering::Relaxed);
        let micros = u64::try_from(duration.as_micros()).unwrap_or(u64::MAX);
        self.sum_micros.fetch_add(micros, Ordering::Relaxed);
    }

    fn render(&self, out: &mut String, name: &str, label_names: &[&str], values: &[String]) {
        let mut cumulative: u64 = 0;
        for (idx, bound) in self.bounds.iter().enumerate() {
            cumulative += self.buckets[idx].load(Ordering::Relaxed);
            write_histogram_bucket(
                out,
                name,
                label_names,
                values,
                &format_f64(*bound),
                cumulative,
            );
        }
        let inf_total = cumulative + self.buckets[self.bounds.len()].load(Ordering::Relaxed);
        write_histogram_bucket(out, name, label_names, values, "+Inf", inf_total);
        let sum_secs = self.sum_micros.load(Ordering::Relaxed) as f64 / 1_000_000.0;
        write_metric_line(
            out,
            &format!("{name}_sum"),
            label_names,
            values,
            format_f64(sum_secs),
        );
        write_metric_line(
            out,
            &format!("{name}_count"),
            label_names,
            values,
            inf_total,
        );
    }
}

struct HistogramVec {
    label_names: &'static [&'static str],
    bounds: &'static [f64],
    series: RwLock<BTreeMap<Vec<String>, Arc<Histogram>>>,
}

impl HistogramVec {
    fn new(label_names: &'static [&'static str], bounds: &'static [f64]) -> Self {
        Self {
            label_names,
            bounds,
            series: RwLock::new(BTreeMap::new()),
        }
    }

    fn observe(&self, labels: &[&str], duration: Duration) {
        debug_assert_eq!(labels.len(), self.label_names.len());
        let key: Vec<String> = labels.iter().map(|s| (*s).to_owned()).collect();
        if let Some(hist) = read_lock(&self.series).get(&key) {
            hist.observe(duration);
            return;
        }
        let mut map = write_lock(&self.series);
        if let Some(hist) = map.get(&key) {
            hist.observe(duration);
            return;
        }
        let target_key = bounded_key(&map, key, self.label_names.len());
        map.entry(target_key)
            .or_insert_with(|| Arc::new(Histogram::new(self.bounds)))
            .observe(duration);
    }

    fn render(&self, out: &mut String, name: &str, help: &str) {
        let _ = writeln!(out, "# HELP {name} {help}");
        let _ = writeln!(out, "# TYPE {name} histogram");
        for (values, hist) in read_lock(&self.series).iter() {
            hist.render(out, name, self.label_names, values);
        }
    }
}

/// 進行中請求的 RAII 守衛：不論正常結束、SSE 串流中止或 panic 都會自動減一。
pub(crate) struct InFlightGuard {
    gauge: Arc<AtomicI64>,
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        self.gauge.fetch_sub(1, Ordering::Relaxed);
    }
}

/// 背景工作的 RAII 守衛：自動維護進行中數量、執行耗時與最終結果（包含未預期 panic／取消）。
pub(crate) struct BackgroundJobGuard {
    metrics: Arc<Metrics>,
    job: &'static str,
    gauge: Option<Arc<AtomicI64>>,
    started: Instant,
    finished: bool,
}

impl BackgroundJobGuard {
    pub(crate) fn finish(mut self, outcome: &'static str) {
        self.record(outcome);
    }

    fn record(&mut self, outcome: &'static str) {
        if self.finished {
            return;
        }
        self.finished = true;
        if let Some(g) = self.gauge.take() {
            g.fetch_sub(1, Ordering::Relaxed);
        }
        self.metrics.background_jobs_total.inc(&[self.job, outcome]);
        self.metrics
            .background_job_duration_seconds
            .observe(&[self.job], self.started.elapsed());
    }
}

impl Drop for BackgroundJobGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.record("aborted");
        }
    }
}

/// 應用程式的記憶體聚合指標登錄。
pub struct Metrics {
    http_requests_in_flight: Arc<AtomicI64>,
    http_requests_total: CounterVec,
    http_requests_aborted_total: CounterVec,
    http_request_errors_total: CounterVec,
    http_request_timeouts_total: CounterVec,
    http_request_duration_seconds: HistogramVec,
    ai_requests_total: CounterVec,
    ai_retries_total: CounterVec,
    ai_request_duration_seconds: HistogramVec,
    background_jobs_in_flight: GaugeVec,
    background_jobs_total: CounterVec,
    background_job_duration_seconds: HistogramVec,
    service_probes_total: CounterVec,
    service_probe_duration_seconds: HistogramVec,
    translations_total: CounterVec,
    translation_duration_seconds: HistogramVec,
    book_source_lookups_total: CounterVec,
    complaint_mails_total: CounterVec,
    auth_events_total: CounterVec,
    diagnostic_writes_dropped_total: CounterVec,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            http_requests_in_flight: Arc::new(AtomicI64::new(0)),
            http_requests_total: CounterVec::new(&["method", "route", "status_class"]),
            http_requests_aborted_total: CounterVec::new(&["method", "route"]),
            http_request_errors_total: CounterVec::new(&["route", "error_code"]),
            http_request_timeouts_total: CounterVec::new(&["route"]),
            http_request_duration_seconds: HistogramVec::new(
                &["method", "route"],
                HTTP_DURATION_BUCKETS,
            ),
            ai_requests_total: CounterVec::new(&["function", "outcome"]),
            ai_retries_total: CounterVec::new(&["function"]),
            ai_request_duration_seconds: HistogramVec::new(&["function"], AI_DURATION_BUCKETS),
            background_jobs_in_flight: GaugeVec::new(&["job"]),
            background_jobs_total: CounterVec::new(&["job", "outcome"]),
            background_job_duration_seconds: HistogramVec::new(&["job"], JOB_DURATION_BUCKETS),
            service_probes_total: CounterVec::new(&["service", "outcome"]),
            service_probe_duration_seconds: HistogramVec::new(&["service"], PROBE_DURATION_BUCKETS),
            translations_total: CounterVec::new(&["provider", "outcome"]),
            translation_duration_seconds: HistogramVec::new(&["provider"], PROBE_DURATION_BUCKETS),
            book_source_lookups_total: CounterVec::new(&["provider", "outcome"]),
            complaint_mails_total: CounterVec::new(&["outcome"]),
            auth_events_total: CounterVec::new(&["method", "outcome"]),
            diagnostic_writes_dropped_total: CounterVec::new(&["anonymous"]),
        }
    }

    pub(crate) fn track_http_in_flight(&self) -> InFlightGuard {
        self.http_requests_in_flight.fetch_add(1, Ordering::Relaxed);
        InFlightGuard {
            gauge: Arc::clone(&self.http_requests_in_flight),
        }
    }

    pub(crate) fn record_http_request(
        &self,
        method: &str,
        route: &str,
        status: u16,
        duration: Duration,
        error_code: Option<&str>,
    ) {
        let class = status_class(status);
        self.http_requests_total.inc(&[method, route, class]);
        self.http_request_duration_seconds
            .observe(&[method, route], duration);
        if let Some(code) = error_code {
            self.http_request_errors_total.inc(&[route, code]);
        }
    }

    pub(crate) fn record_http_aborted(&self, method: &str, route: &str) {
        self.http_requests_aborted_total.inc(&[method, route]);
    }

    pub(crate) fn record_http_timeout(&self, route: &str) {
        self.http_request_timeouts_total.inc(&[route]);
    }

    pub(crate) fn record_ai(
        &self,
        function: &str,
        outcome: &str,
        retries: i32,
        duration: Duration,
    ) {
        self.ai_requests_total.inc(&[function, outcome]);
        if retries > 0 {
            self.ai_retries_total.inc_by(&[function], retries as u64);
        }
        self.ai_request_duration_seconds
            .observe(&[function], duration);
    }

    pub(crate) fn track_background_job(self: &Arc<Self>, job: &'static str) -> BackgroundJobGuard {
        let gauge = self.background_jobs_in_flight.get_or_create(&[job]);
        if let Some(g) = &gauge {
            g.fetch_add(1, Ordering::Relaxed);
        }
        BackgroundJobGuard {
            metrics: Arc::clone(self),
            job,
            gauge,
            started: Instant::now(),
            finished: false,
        }
    }

    pub(crate) fn record_service_probe(&self, service: &str, outcome: &str, duration: Duration) {
        self.service_probes_total.inc(&[service, outcome]);
        self.service_probe_duration_seconds
            .observe(&[service], duration);
    }

    pub(crate) fn record_translation(&self, provider: &str, outcome: &str, duration: Duration) {
        self.translations_total.inc(&[provider, outcome]);
        self.translation_duration_seconds
            .observe(&[provider], duration);
    }

    pub(crate) fn record_book_source_lookup(&self, provider: &str, outcome: &str) {
        self.book_source_lookups_total.inc(&[provider, outcome]);
    }

    pub(crate) fn record_complaint_mail(&self, outcome: &str) {
        self.complaint_mails_total.inc(&[outcome]);
    }

    pub(crate) fn record_auth_event(&self, method: &str, outcome: &str) {
        self.auth_events_total.inc(&[method, outcome]);
    }

    pub(crate) fn record_diagnostic_write_dropped(&self, anonymous: bool) {
        self.diagnostic_writes_dropped_total
            .inc(&[if anonymous { "true" } else { "false" }]);
    }

    /// 匯出符合 Prometheus Text Exposition Format 0.0.4 的所有指標。
    pub fn render(&self, state: &AppState) -> String {
        let mut out = String::with_capacity(4096);

        // Process & build gauges (sampled live at scrape time)
        let _ = writeln!(out, "# HELP socrates_build_info Build version information.");
        let _ = writeln!(out, "# TYPE socrates_build_info gauge");
        write_metric_line(
            &mut out,
            "socrates_build_info",
            &["version"],
            std::slice::from_ref(&state.version),
            1,
        );

        let uptime = (Utc::now() - state.process_started_at).num_seconds().max(0);
        write_scalar_gauge(
            &mut out,
            "socrates_process_uptime_seconds",
            "Process uptime in seconds.",
            uptime,
        );

        write_scalar_gauge(
            &mut out,
            "socrates_maintenance_mode",
            "Whether maintenance mode is enabled in the cached flag (1=on, 0=off).",
            i64::from(state.maintenance.cached_is_on()),
        );

        write_scalar_gauge(
            &mut out,
            "socrates_last_cleanup_timestamp_seconds",
            "Unix timestamp of the last successful diagnostic retention cleanup (0 if never).",
            state.last_cleanup.load(Ordering::Relaxed),
        );

        // Live database pool gauges
        let pool_size = i64::from(state.pool.size());
        let pool_idle = i64::try_from(state.pool.num_idle()).unwrap_or(i64::MAX);
        let pool_active = (pool_size - pool_idle).max(0);
        let pool_max = i64::from(state.pool.options().get_max_connections());
        let _ = writeln!(
            out,
            "# HELP socrates_db_pool_connections Database connection pool counts by state."
        );
        let _ = writeln!(out, "# TYPE socrates_db_pool_connections gauge");
        for (label, value) in [
            ("active", pool_active),
            ("idle", pool_idle),
            ("max", pool_max),
            ("size", pool_size),
        ] {
            write_metric_line(
                &mut out,
                "socrates_db_pool_connections",
                &["state"],
                &[label.to_owned()],
                value,
            );
        }

        // Live session gauges
        write_scalar_gauge(
            &mut out,
            "socrates_live_replies_active",
            "Number of active streaming AI reply generations.",
            i64::try_from(state.live.active_count()).unwrap_or(i64::MAX),
        );
        write_scalar_gauge(
            &mut out,
            "socrates_voice_sessions_active",
            "Number of active realtime voice sessions.",
            i64::try_from(state.voice_sessions.active_count()).unwrap_or(i64::MAX),
        );

        // Live monitored service status gauges
        let _ = writeln!(
            out,
            "# HELP socrates_service_up Current monitored service health status (1=healthy, 0=not healthy)."
        );
        let _ = writeln!(out, "# TYPE socrates_service_up gauge");
        for (service, healthy) in state.service_monitor.statuses() {
            write_metric_line(
                &mut out,
                "socrates_service_up",
                &["service"],
                &[service],
                i64::from(healthy),
            );
        }

        // HTTP metrics
        write_scalar_gauge(
            &mut out,
            "socrates_http_requests_in_flight",
            "Current number of in-flight HTTP requests (including open SSE streams).",
            self.http_requests_in_flight.load(Ordering::Relaxed),
        );
        self.http_requests_total.render(
            &mut out,
            "socrates_http_requests_total",
            "Total HTTP requests completed by method, route template, and status class.",
        );
        self.http_requests_aborted_total.render(
            &mut out,
            "socrates_http_requests_aborted_total",
            "Total HTTP requests aborted before response headers were sent by method and route template.",
        );
        self.http_request_errors_total.render(
            &mut out,
            "socrates_http_request_errors_total",
            "Total HTTP requests resulting in a structured API or SSE error code.",
        );
        self.http_request_timeouts_total.render(
            &mut out,
            "socrates_http_request_timeouts_total",
            "Total HTTP requests rejected by the request timeout middleware.",
        );
        self.http_request_duration_seconds.render(
            &mut out,
            "socrates_http_request_duration_seconds",
            "HTTP request latency distribution in seconds by method and route template.",
        );

        // AI metrics
        self.ai_requests_total.render(
            &mut out,
            "socrates_ai_requests_total",
            "Total AI provider calls by function and outcome.",
        );
        self.ai_retries_total.render(
            &mut out,
            "socrates_ai_retries_total",
            "Total AI provider retries by function.",
        );
        self.ai_request_duration_seconds.render(
            &mut out,
            "socrates_ai_request_duration_seconds",
            "AI provider call latency distribution in seconds by function.",
        );

        // Background job metrics
        self.background_jobs_in_flight.render(
            &mut out,
            "socrates_background_jobs_in_flight",
            "Current number of in-flight background jobs by job type.",
        );
        self.background_jobs_total.render(
            &mut out,
            "socrates_background_jobs_total",
            "Total completed background jobs by job type and outcome.",
        );
        self.background_job_duration_seconds.render(
            &mut out,
            "socrates_background_job_duration_seconds",
            "Background job execution duration in seconds by job type.",
        );

        // Service probe & external dependency metrics
        self.service_probes_total.render(
            &mut out,
            "socrates_service_probes_total",
            "Total monitored service health probes by service name and outcome.",
        );
        self.service_probe_duration_seconds.render(
            &mut out,
            "socrates_service_probe_duration_seconds",
            "Monitored service health probe latency in seconds by service name.",
        );
        self.translations_total.render(
            &mut out,
            "socrates_translations_total",
            "Total external translation requests by provider and outcome.",
        );
        self.translation_duration_seconds.render(
            &mut out,
            "socrates_translation_duration_seconds",
            "External translation request latency in seconds by provider.",
        );
        self.book_source_lookups_total.render(
            &mut out,
            "socrates_book_source_lookups_total",
            "Total external book source lookups by provider and outcome.",
        );
        self.complaint_mails_total.render(
            &mut out,
            "socrates_complaint_mails_total",
            "Total complaint notification emails processed by outcome.",
        );
        self.auth_events_total.render(
            &mut out,
            "socrates_auth_events_total",
            "Total authentication outcomes by method and outcome.",
        );
        self.diagnostic_writes_dropped_total.render(
            &mut out,
            "socrates_diagnostic_writes_dropped_total",
            "Total diagnostic writes dropped due to write backlog.",
        );

        out
    }

    #[cfg(test)]
    pub(crate) fn render_recorded(&self) -> String {
        let mut out = String::new();
        write_scalar_gauge(
            &mut out,
            "socrates_http_requests_in_flight",
            "help",
            self.http_requests_in_flight.load(Ordering::Relaxed),
        );
        self.http_requests_total
            .render(&mut out, "socrates_http_requests_total", "help");
        self.http_requests_aborted_total.render(
            &mut out,
            "socrates_http_requests_aborted_total",
            "help",
        );
        self.translations_total
            .render(&mut out, "socrates_translations_total", "help");
        self.translation_duration_seconds.render(
            &mut out,
            "socrates_translation_duration_seconds",
            "help",
        );
        self.background_jobs_in_flight.render(
            &mut out,
            "socrates_background_jobs_in_flight",
            "help",
        );
        self.background_jobs_total
            .render(&mut out, "socrates_background_jobs_total", "help");
        self.background_job_duration_seconds.render(
            &mut out,
            "socrates_background_job_duration_seconds",
            "help",
        );
        self.service_probes_total
            .render(&mut out, "socrates_service_probes_total", "help");
        out
    }
}

fn status_class(status: u16) -> &'static str {
    match status {
        100..=199 => "1xx",
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        500..=599 => "5xx",
        _ => "other",
    }
}

fn escape_label_value(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out
}

fn format_f64(value: f64) -> String {
    let s = format!("{value:.6}");
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() {
        "0".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn write_scalar_gauge(out: &mut String, name: &str, help: &str, value: i64) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} gauge");
    let _ = writeln!(out, "{name} {value}");
}

fn write_metric_line(
    out: &mut String,
    name: &str,
    label_names: &[&str],
    values: &[String],
    value: impl std::fmt::Display,
) {
    let _ = write!(out, "{name}{{");
    for (i, (k, v)) in label_names.iter().zip(values.iter()).enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, "{k}=\"{}\"", escape_label_value(v));
    }
    let _ = writeln!(out, "}} {value}");
}

fn write_histogram_bucket(
    out: &mut String,
    name: &str,
    label_names: &[&str],
    values: &[String],
    le: &str,
    count: u64,
) {
    let _ = write!(out, "{name}_bucket{{");
    for (k, v) in label_names.iter().zip(values.iter()) {
        let _ = write!(out, "{k}=\"{}\",", escape_label_value(v));
    }
    let _ = writeln!(out, "le=\"{le}\"}} {count}");
}

/// 以固定時間比對 `Authorization: Bearer <token>` 是否符合 `METRICS_TOKEN`（依 RFC 6750／9110 不區分 scheme 大小寫）。
pub(crate) fn has_valid_metrics_token(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(expected) = state.config.metrics_token.as_deref() else {
        return false;
    };
    let Some(given) = extract_bearer_token(headers) else {
        return false;
    };
    constant_time_eq_sha256(given.as_bytes(), expected.as_bytes())
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = raw.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() {
        return None;
    }
    Some(token)
}

fn constant_time_eq_sha256(a: &[u8], b: &[u8]) -> bool {
    let da = Sha256::digest(a);
    let db = Sha256::digest(b);
    let diff = da
        .iter()
        .zip(db.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y));
    diff == 0
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/metrics", get(scrape))
        .route_layer(axum::middleware::map_response(
            |mut response: Response| async move {
                response
                    .headers_mut()
                    .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
                response
            },
        ))
}

/// Prometheus 文字格式指標端點（僅系統管理者或設定了 `METRICS_TOKEN` 的 Bearer 憑證可用）。
#[utoipa::path(
    get,
    path = "/api/admin/metrics",
    tag = "admin",
    summary = "匯出 Prometheus 系統指標",
    description = "以 Prometheus 文字格式（0.0.4）匯出 HTTP、AI、背景工作、連線池與周邊服務的 Counter／Gauge／Histogram 指標。限系統管理者或設定了 `METRICS_TOKEN` 的 Bearer 憑證使用，不含任何個資或對話原文。",
    responses(
        (status = 200, description = "Prometheus 文字格式指標", content_type = "text/plain; version=0.0.4; charset=utf-8", body = String),
        (status = 401, description = "未登入或 Bearer token 無效", body = ErrorBody),
        (status = 403, description = "非系統管理者", body = ErrorBody)
    )
)]
pub(crate) async fn scrape(
    State(state): State<AppState>,
    req: Request,
) -> Result<Response, ApiError> {
    let (mut parts, _) = req.into_parts();
    if !has_valid_metrics_token(&state, &parts.headers) {
        let _admin = RequireAdmin::from_request_parts(&mut parts, &state).await?;
    }
    let body = state.metrics.render(&state);
    Ok(([(header::CONTENT_TYPE, PROMETHEUS_CONTENT_TYPE)], body).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_histogram_and_label_escaping_render_valid_prometheus() {
        let m = Metrics::new();
        m.record_http_request("GET", "/api/topics", 200, Duration::from_millis(42), None);
        m.record_http_request(
            "GET",
            "/api/topics",
            500,
            Duration::from_millis(1200),
            Some("internal"),
        );
        m.record_http_request(
            "POST",
            "/api/weird\"path\\line\nbreak",
            400,
            Duration::from_secs(45),
            Some("bad_request"),
        );

        let mut out = String::new();
        m.http_requests_total
            .render(&mut out, "socrates_http_requests_total", "help");
        assert!(out.contains(
            "socrates_http_requests_total{method=\"GET\",route=\"/api/topics\",status_class=\"2xx\"} 1"
        ));
        assert!(out.contains(
            "socrates_http_requests_total{method=\"GET\",route=\"/api/topics\",status_class=\"5xx\"} 1"
        ));
        // Label values are escaped cleanly per Prometheus text format
        assert!(out.contains(r#"route="/api/weird\"path\\line\nbreak""#));

        let mut hist_out = String::new();
        m.http_request_duration_seconds.render(
            &mut hist_out,
            "socrates_http_request_duration_seconds",
            "help",
        );
        // 42ms falls in <= 0.05s bucket; 1200ms falls in <= 2.5s bucket
        assert!(hist_out.contains(
            "socrates_http_request_duration_seconds_bucket{method=\"GET\",route=\"/api/topics\",le=\"0.025\"} 0"
        ));
        assert!(hist_out.contains(
            "socrates_http_request_duration_seconds_bucket{method=\"GET\",route=\"/api/topics\",le=\"0.05\"} 1"
        ));
        assert!(hist_out.contains(
            "socrates_http_request_duration_seconds_bucket{method=\"GET\",route=\"/api/topics\",le=\"1\"} 1"
        ));
        assert!(hist_out.contains(
            "socrates_http_request_duration_seconds_bucket{method=\"GET\",route=\"/api/topics\",le=\"2.5\"} 2"
        ));
        assert!(hist_out.contains(
            "socrates_http_request_duration_seconds_bucket{method=\"GET\",route=\"/api/topics\",le=\"+Inf\"} 2"
        ));
        assert!(hist_out.contains(
            "socrates_http_request_duration_seconds_count{method=\"GET\",route=\"/api/topics\"} 2"
        ));
        assert!(hist_out.contains(
            "socrates_http_request_duration_seconds_sum{method=\"GET\",route=\"/api/topics\"} 1.242"
        ));
        // 45s exceeds max bucket (30s) and lands only in +Inf
        assert!(hist_out.contains(
            "socrates_http_request_duration_seconds_bucket{method=\"POST\",route=\"/api/weird\\\"path\\\\line\\nbreak\",le=\"30\"} 0"
        ));
        assert!(hist_out.contains(
            "socrates_http_request_duration_seconds_bucket{method=\"POST\",route=\"/api/weird\\\"path\\\\line\\nbreak\",le=\"+Inf\"} 1"
        ));
    }

    #[test]
    fn in_flight_and_background_job_guards_clean_up_on_drop_and_panic() {
        let m = Arc::new(Metrics::new());
        {
            let _g1 = m.track_http_in_flight();
            let _g2 = m.track_http_in_flight();
            assert_eq!(m.http_requests_in_flight.load(Ordering::Relaxed), 2);
        }
        assert_eq!(m.http_requests_in_flight.load(Ordering::Relaxed), 0);

        // Background job explicit finish
        let job = m.track_background_job("summary");
        job.finish("ok");

        // Background job dropped unexpectedly (simulating panic/cancellation)
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _job = m.track_background_job("reply");
            panic!("boom");
        }));

        let mut jobs = String::new();
        m.background_jobs_in_flight
            .render(&mut jobs, "socrates_background_jobs_in_flight", "help");
        assert!(jobs.contains("socrates_background_jobs_in_flight{job=\"summary\"} 0"));
        assert!(jobs.contains("socrates_background_jobs_in_flight{job=\"reply\"} 0"));

        let mut totals = String::new();
        m.background_jobs_total
            .render(&mut totals, "socrates_background_jobs_total", "help");
        assert!(
            totals.contains("socrates_background_jobs_total{job=\"summary\",outcome=\"ok\"} 1")
        );
        assert!(
            totals.contains("socrates_background_jobs_total{job=\"reply\",outcome=\"aborted\"} 1")
        );
    }

    #[test]
    fn cardinality_cap_aggregates_excess_series_into_overflow_bucket() {
        let vec = CounterVec::new(&["route"]);
        let total = MAX_SERIES + 50;
        for i in 0..total {
            vec.inc(&[&format!("/route/{i}")]);
        }
        assert_eq!(read_lock(&vec.series).len(), MAX_SERIES);
        let mut out = String::new();
        vec.render(&mut out, "test_requests_total", "help");
        // MAX_SERIES - 1 distinct routes + 51 overflowed into <overflow>
        assert!(
            out.contains("test_requests_total{route=\"<overflow>\"} 51"),
            "expected overflow bucket with exact aggregated count: {out}"
        );

        let hvec = HistogramVec::new(&["route"], HTTP_DURATION_BUCKETS);
        for i in 0..total {
            hvec.observe(&[&format!("/route/{i}")], Duration::from_millis(10));
        }
        assert_eq!(read_lock(&hvec.series).len(), MAX_SERIES);
        let mut hout = String::new();
        hvec.render(&mut hout, "test_duration_seconds", "help");
        assert!(hout.contains("test_duration_seconds_count{route=\"<overflow>\"} 51"));
    }

    #[test]
    fn histogram_inf_bucket_always_matches_count() {
        let hist = Arc::new(Histogram::new(HTTP_DURATION_BUCKETS));
        let writer = {
            let hist = Arc::clone(&hist);
            std::thread::spawn(move || {
                for i in 0..2000u64 {
                    hist.observe(Duration::from_millis(i % 500));
                }
            })
        };
        for _ in 0..200 {
            let mut out = String::new();
            hist.render(&mut out, "h", &["route"], &["/health".to_owned()]);
            let inf = out
                .lines()
                .find(|l| l.contains("le=\"+Inf\""))
                .and_then(|l| l.split_whitespace().last())
                .unwrap();
            let count = out
                .lines()
                .find(|l| l.starts_with("h_count"))
                .and_then(|l| l.split_whitespace().last())
                .unwrap();
            assert_eq!(inf, count, "histogram +Inf must equal _count: {out}");
        }
        writer.join().unwrap();
    }

    #[test]
    fn bearer_token_extraction_is_case_insensitive_on_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("bearer my-token"),
        );
        assert_eq!(extract_bearer_token(&headers), Some("my-token"));

        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("BEARER My-Case-Token"),
        );
        assert_eq!(extract_bearer_token(&headers), Some("My-Case-Token"));

        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Basic dXNlcjpwYXNz"),
        );
        assert_eq!(extract_bearer_token(&headers), None);

        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer "));
        assert_eq!(extract_bearer_token(&headers), None);
    }

    #[test]
    fn constant_time_sha256_comparison_checks_exact_equality() {
        assert!(constant_time_eq_sha256(b"secret-token", b"secret-token"));
        assert!(!constant_time_eq_sha256(b"secret-token", b"secret-tokeN"));
        assert!(!constant_time_eq_sha256(b"secret-token", b""));
        assert!(!constant_time_eq_sha256(b"", b"secret-token"));
    }
}
