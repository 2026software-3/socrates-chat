//! 每個請求完成時輸出一筆 `request completed` 的 tracing event（stdout 是完整請求日誌的來源）。
//! 串流回應的 error code 要等本文結束才知道，所以延後到本文結束或被丟棄時才輸出。

use std::{sync::Arc, time::Instant};

use tracing::Span;

use crate::metrics::{InFlightGuard, Metrics};

/// 沒有對應路由的請求，記錄時的 route 名稱。
pub(crate) const FALLBACK_ROUTE: &str = "<fallback>";

/// 一個已完成的請求；只含路由模板與固定的 error code，不含原始 URL、header 或 body。
pub(crate) struct RequestLogEntry {
    pub request_id: String,
    pub trace_id: String,
    pub span_id: String,
    pub method: String,
    pub route: String,
    pub status: u16,
    pub duration_ms: u64,
    pub error_code: Option<String>,
}

/// 輸出一筆已完成請求的 tracing event；日誌等級依結果區分（5xx → error、4xx 或串流錯誤 → warn、其餘 → info）。
pub(crate) fn record(span: &Span, entry: &RequestLogEntry) {
    span.in_scope(|| {
        if entry.status >= 500 {
            tracing::error!(
                request_id = %entry.request_id,
                trace_id = %entry.trace_id,
                span_id = %entry.span_id,
                method = %entry.method,
                route = %entry.route,
                status = entry.status,
                duration_ms = entry.duration_ms,
                error_code = entry.error_code.as_deref(),
                "request completed"
            );
        } else if entry.status >= 400 || entry.error_code.is_some() {
            tracing::warn!(
                request_id = %entry.request_id,
                trace_id = %entry.trace_id,
                span_id = %entry.span_id,
                method = %entry.method,
                route = %entry.route,
                status = entry.status,
                duration_ms = entry.duration_ms,
                error_code = entry.error_code.as_deref(),
                "request completed"
            );
        } else {
            tracing::info!(
                request_id = %entry.request_id,
                trace_id = %entry.trace_id,
                span_id = %entry.span_id,
                method = %entry.method,
                route = %entry.route,
                status = entry.status,
                duration_ms = entry.duration_ms,
                // None 時不輸出這個欄位，與 API 的 null 一致
                error_code = entry.error_code.as_deref(),
                "request completed"
            );
        }
    });
}

/// 一個尚未收到結尾空行的事件最多保留的位元組數；超過就丟棄（`error` 事件只有幾十個位元組）。
const MAX_PENDING: usize = 64 * 1024;

/// 串流回應的結果：本文中最後一個 `error` 事件的 code。`done` 與其他事件沒有 error code。
#[derive(Default)]
pub(crate) struct SseOutcome {
    /// 尚未收滿一個事件（以空行結束）的位元組；事件可能跨越多個片段。
    pending: Vec<u8>,
    /// `pending` 中已經找過空行的長度，下一個片段從這裡接著找，不必從頭重掃。
    scanned: usize,
    error_code: Option<String>,
}

impl SseOutcome {
    pub fn observe(&mut self, chunk: &[u8]) {
        self.pending.extend_from_slice(chunk);
        // 空行可能跨在兩個片段之間，所以從上次掃描位置的前一個位元組開始找
        while let Some(i) = self.pending[self.scanned.saturating_sub(1)..]
            .windows(2)
            .position(|w| w == b"\n\n")
        {
            let end = self.scanned.saturating_sub(1) + i;
            if let Some(code) = error_code_of(&String::from_utf8_lossy(&self.pending[..end])) {
                self.error_code = Some(code);
            }
            self.pending.drain(..end + 2);
            self.scanned = 0;
        }
        self.scanned = self.pending.len();
        if self.pending.len() > MAX_PENDING {
            self.pending.clear();
            self.scanned = 0;
        }
    }

    pub fn error_code(&self) -> Option<&str> {
        self.error_code.as_deref()
    }
}

/// 若這個事件是 `error`，回傳它的 code（缺少或無法解析時為 `"error"`）。
fn error_code_of(event: &str) -> Option<String> {
    let mut name = None;
    let mut data = None;
    for line in event.lines() {
        if let Some(v) = line.strip_prefix("event:") {
            name = Some(v.trim());
        } else if let Some(v) = line.strip_prefix("data:") {
            data = Some(v.trim());
        }
    }
    if name != Some("error") {
        return None;
    }
    Some(
        data.and_then(|d| serde_json::from_str::<serde_json::Value>(d).ok())
            .and_then(|v| v["code"].as_str().map(str::to_string))
            .unwrap_or_else(|| "error".to_string()),
    )
}

/// 串流回應的 request log 與指標：耗時與 error code 只有在本文結束（或連線中斷、本文被丟棄）時才確定，
/// 因此在 `Drop` 時才輸出日誌、寫入指標並釋放 `in_flight` 守衛；傳入的 entry 其中的 `duration_ms`、`error_code` 會被覆寫。
pub(crate) struct StreamLog {
    span: Span,
    started: Instant,
    /// 結束時取出補上耗時與 error code 後輸出，因此不必複製。
    entry: Option<RequestLogEntry>,
    outcome: SseOutcome,
    metrics: Arc<Metrics>,
    _in_flight: InFlightGuard,
}

impl StreamLog {
    pub fn new(
        span: Span,
        started: Instant,
        entry: RequestLogEntry,
        metrics: Arc<Metrics>,
        in_flight: InFlightGuard,
    ) -> Self {
        Self {
            span,
            started,
            entry: Some(entry),
            outcome: SseOutcome::default(),
            metrics,
            _in_flight: in_flight,
        }
    }

    pub fn observe(&mut self, chunk: &[u8]) {
        self.outcome.observe(chunk);
    }
}

impl Drop for StreamLog {
    fn drop(&mut self) {
        let Some(mut entry) = self.entry.take() else {
            return;
        };
        let elapsed = self.started.elapsed();
        entry.duration_ms = elapsed.as_millis() as u64;
        entry.error_code = self.outcome.error_code().map(str::to_string);
        self.metrics.record_http_request(
            &entry.method,
            &entry.route,
            entry.status,
            elapsed,
            entry.error_code.as_deref(),
        );
        record(&self.span, &entry);
    }
}

/// 非 SSE 請求處理期間的 RAII 守衛：持有 `InFlightGuard`，正常完成或交棒給 `StreamLog` 時
/// 透過 [`Self::disarm`] 解除武裝；若用戶端在回應產生前斷線或發生 panic 導致 future 被丟棄，
/// 會在 `Drop` 時寫出結構化的 `request aborted` 警告日誌、累加 `socrates_http_requests_aborted_total`
/// 並釋放 `in_flight`。
pub(crate) struct RequestGuard {
    span: Span,
    started: Instant,
    entry: Option<RequestLogEntry>,
    metrics: Arc<Metrics>,
    in_flight: Option<InFlightGuard>,
}

impl RequestGuard {
    pub fn new(
        span: Span,
        started: Instant,
        entry: RequestLogEntry,
        metrics: Arc<Metrics>,
        in_flight: InFlightGuard,
    ) -> Self {
        Self {
            span,
            started,
            entry: Some(entry),
            metrics,
            in_flight: Some(in_flight),
        }
    }

    /// 正常完成或交棒給 `StreamLog` 時呼叫：解除中止追蹤並取回 `InFlightGuard` 與請求中繼資料。
    pub fn disarm(mut self) -> (InFlightGuard, RequestLogEntry) {
        let in_flight = self
            .in_flight
            .take()
            .expect("RequestGuard disarmed at most once");
        let entry = self
            .entry
            .take()
            .expect("RequestGuard disarmed at most once");
        (in_flight, entry)
    }
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        let Some(_in_flight) = self.in_flight.take() else {
            return;
        };
        let Some(entry) = self.entry.take() else {
            return;
        };
        let duration_ms = self.started.elapsed().as_millis() as u64;
        self.metrics
            .record_http_aborted(&entry.method, &entry.route);
        let panicking = std::thread::panicking();
        let reason = if panicking {
            "panic"
        } else {
            "client_disconnected"
        };
        self.span.in_scope(|| {
            if panicking {
                tracing::error!(
                    request_id = %entry.request_id,
                    trace_id = %entry.trace_id,
                    span_id = %entry.span_id,
                    method = %entry.method,
                    route = %entry.route,
                    duration_ms,
                    reason,
                    "request aborted"
                );
            } else {
                tracing::warn!(
                    request_id = %entry.request_id,
                    trace_id = %entry.trace_id,
                    span_id = %entry.span_id,
                    method = %entry.method,
                    route = %entry.route,
                    duration_ms,
                    reason,
                    "request aborted"
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::{io, sync::Mutex};

    use serde_json::Value;
    use tracing_subscriber::fmt::MakeWriter;

    use super::*;

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    struct Sink(Arc<Mutex<Vec<u8>>>);

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

    #[test]
    fn stream_outcome_reads_the_error_code_across_chunk_boundaries() {
        let mut outcome = SseOutcome::default();
        for chunk in [
            &b"event: delta\ndata: {\"text\":\"x\"}\n\n"[..],
            b"event: error\nda",
            b"ta: {\"code\":\"ai_unavailable\"}\n\n",
        ] {
            outcome.observe(chunk);
        }
        assert_eq!(outcome.error_code(), Some("ai_unavailable"));
    }

    #[test]
    fn stream_outcome_does_not_buffer_an_event_that_never_ends() {
        let mut outcome = SseOutcome::default();
        for _ in 0..100 {
            outcome.observe(&[b'x'; 4096]);
        }
        assert!(outcome.pending.len() <= 64 * 1024);
        outcome.observe(b"\n\nevent: error\ndata: {\"code\":\"internal\"}\n\n");
        assert_eq!(outcome.error_code(), Some("internal"));
    }

    #[test]
    fn stream_without_an_error_event_has_no_error_code() {
        let mut outcome = SseOutcome::default();
        outcome.observe(b": keep-alive\n\n");
        outcome.observe(b"event: done\ndata: {}\n\n");
        assert_eq!(outcome.error_code(), None);
    }

    #[test]
    fn record_uses_severity_matching_outcome_and_includes_trace_fields() {
        let captured = Captured::default();
        let subscriber = crate::logging::json_subscriber()
            .with_env_filter(crate::logging::env_filter(Some("info")))
            .with_writer(captured.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let span = tracing::info_span!("http_request");

        for (status, error_code) in [
            (200, None),
            (404, Some("not_found")),
            (500, Some("internal")),
            (200, Some("ai_unavailable")),
        ] {
            record(
                &span,
                &RequestLogEntry {
                    request_id: "req-1".into(),
                    trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".into(),
                    span_id: "00f067aa0ba902b7".into(),
                    method: "GET".into(),
                    route: "/api/test".into(),
                    status,
                    duration_ms: 12,
                    error_code: error_code.map(str::to_string),
                },
            );
        }

        let out = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<Value> = out
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0]["level"], "INFO");
        assert_eq!(lines[1]["level"], "WARN");
        assert_eq!(lines[2]["level"], "ERROR");
        assert_eq!(lines[3]["level"], "WARN");
        for line in &lines {
            assert_eq!(
                line["fields"]["trace_id"],
                "4bf92f3577b34da6a3ce929d0e0e4736"
            );
            assert_eq!(line["fields"]["span_id"], "00f067aa0ba902b7");
        }
    }

    #[test]
    fn request_guard_records_abort_only_when_dropped_without_disarm() {
        let captured = Captured::default();
        let subscriber = crate::logging::json_subscriber()
            .with_env_filter(crate::logging::env_filter(Some("info")))
            .with_writer(captured.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let metrics = Arc::new(Metrics::new());
        let span = tracing::info_span!("http_request");
        let make_entry = |route: &str| RequestLogEntry {
            request_id: "req-guard".into(),
            trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".into(),
            span_id: "00f067aa0ba902b7".into(),
            method: "POST".into(),
            route: route.into(),
            status: 0,
            duration_ms: 0,
            error_code: None,
        };

        // 1. Disarmed guard must NOT log "request aborted"
        let g_ok = RequestGuard::new(
            span.clone(),
            Instant::now(),
            make_entry("/api/ok"),
            metrics.clone(),
            metrics.track_http_in_flight(),
        );
        let (in_flight, entry) = g_ok.disarm();
        assert_eq!(entry.route, "/api/ok");
        drop(in_flight);

        // 2. Dropped armed guard logs WARN "request aborted" with reason=client_disconnected
        {
            let _g_aborted = RequestGuard::new(
                span.clone(),
                Instant::now(),
                make_entry("/api/aborted"),
                metrics.clone(),
                metrics.track_http_in_flight(),
            );
        }

        // 3. Unwinding armed guard logs ERROR "request aborted" with reason=panic
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g_panicked = RequestGuard::new(
                span,
                Instant::now(),
                make_entry("/api/panicked"),
                metrics.clone(),
                metrics.track_http_in_flight(),
            );
            panic!("SECRET-GUARD-PANIC-PAYLOAD");
        }));
        assert!(panicked.is_err());

        let out = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        assert!(!out.contains("SECRET-GUARD-PANIC-PAYLOAD"));
        let lines: Vec<Value> = out
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["level"], "WARN");
        assert_eq!(lines[0]["fields"]["message"], "request aborted");
        assert_eq!(lines[0]["fields"]["reason"], "client_disconnected");
        assert_eq!(lines[0]["fields"]["route"], "/api/aborted");
        assert_eq!(lines[0]["fields"]["method"], "POST");
        assert_eq!(
            lines[0]["fields"]["trace_id"],
            "4bf92f3577b34da6a3ce929d0e0e4736"
        );
        assert_eq!(lines[0]["fields"]["span_id"], "00f067aa0ba902b7");
        assert!(lines[0]["fields"]["duration_ms"].is_u64());

        assert_eq!(lines[1]["level"], "ERROR");
        assert_eq!(lines[1]["fields"]["message"], "request aborted");
        assert_eq!(lines[1]["fields"]["reason"], "panic");
        assert_eq!(lines[1]["fields"]["route"], "/api/panicked");

        let prom = metrics.render_recorded();
        assert!(prom.contains("socrates_http_requests_in_flight 0"));
        assert!(prom.contains(
            "socrates_http_requests_aborted_total{method=\"POST\",route=\"/api/aborted\"} 1"
        ));
        assert!(prom.contains(
            "socrates_http_requests_aborted_total{method=\"POST\",route=\"/api/panicked\"} 1"
        ));
        assert!(!prom.contains("route=\"/api/ok\""));
    }
}
