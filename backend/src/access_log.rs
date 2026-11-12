//! 每個請求完成時輸出一筆 `request completed` 的 tracing event（stdout 是完整請求日誌的來源）。
//! 串流回應的 error code 要等本文結束才知道，所以延後到本文結束或被丟棄時才輸出。

use std::time::Instant;

use tracing::Span;

/// 沒有對應路由的請求，記錄時的 route 名稱。
pub(crate) const FALLBACK_ROUTE: &str = "<fallback>";

/// 一個已完成的請求；只含路由模板與固定的 error code，不含原始 URL、header 或 body。
pub(crate) struct RequestLogEntry {
    pub request_id: String,
    pub method: String,
    pub route: String,
    pub status: u16,
    pub duration_ms: u64,
    pub error_code: Option<String>,
}

/// 輸出一筆已完成請求的 tracing event。
pub(crate) fn record(span: &Span, entry: &RequestLogEntry) {
    span.in_scope(|| {
        tracing::info!(
            request_id = %entry.request_id,
            method = %entry.method,
            route = %entry.route,
            status = entry.status,
            duration_ms = entry.duration_ms,
            // None 時不輸出這個欄位，與 API 的 null 一致
            error_code = entry.error_code.as_deref(),
            "request completed"
        );
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

/// 串流回應的 request log：耗時與 error code 只有在本文結束（或連線中斷、本文被丟棄）時才確定，
/// 因此在 `Drop` 時才輸出；傳入的 entry 其中的 `duration_ms`、`error_code` 會被覆寫。
pub(crate) struct StreamLog {
    span: Span,
    started: Instant,
    /// 結束時取出補上耗時與 error code 後輸出，因此不必複製。
    entry: Option<RequestLogEntry>,
    outcome: SseOutcome,
}

impl StreamLog {
    pub fn new(span: Span, started: Instant, entry: RequestLogEntry) -> Self {
        Self {
            span,
            started,
            entry: Some(entry),
            outcome: SseOutcome::default(),
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
        entry.duration_ms = self.started.elapsed().as_millis() as u64;
        entry.error_code = self.outcome.error_code().map(str::to_string);
        record(&self.span, &entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
