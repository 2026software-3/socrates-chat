//! 管理者暫時維運頁使用的記憶體 request log；程序重啟即清空。

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Instant,
};

use axum::{
    Json, Router,
    extract::{Request, State},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use tracing::Span;

use crate::{AppState, auth::RequireAdmin, mutex::lock};

pub const CAPACITY: usize = 500;

/// 沒有對應路由的請求，記錄時的 route 名稱。
pub(crate) const FALLBACK_ROUTE: &str = "<fallback>";

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct RequestLogEntry {
    pub created_at: DateTime<Utc>,
    pub request_id: String,
    pub method: String,
    pub route: String,
    pub status: u16,
    pub duration_ms: u64,
    pub error_code: Option<String>,
}

/// 條目以 `Arc` 保存：讀取 ring 只複製指標，不必在持鎖時複製每筆的字串。
#[derive(Default)]
pub(crate) struct RequestLogStore(Mutex<VecDeque<Arc<RequestLogEntry>>>);

impl RequestLogStore {
    pub fn push(&self, entry: RequestLogEntry) {
        let mut entries = lock(&self.0);
        if entries.len() == CAPACITY {
            entries.pop_front();
        }
        entries.push_back(Arc::new(entry));
    }

    fn recent(&self) -> Vec<Arc<RequestLogEntry>> {
        lock(&self.0).iter().rev().cloned().collect()
    }
}

/// 寫入一筆已完成的 request：所有 request 都輸出完整的 tracing event；`in_ring` 為真才進入 ring。
pub(crate) fn record(store: &RequestLogStore, span: &Span, entry: RequestLogEntry, in_ring: bool) {
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
    if in_ring {
        store.push(entry);
    }
}

/// 回應上的標記：這個回應不進 ring（stdout 的 tracing event 仍會輸出）。
#[derive(Clone, Copy)]
struct NotInRing;

/// 標記這個回應不進 ring。路由或處理函式依自己的性質呼叫，共用的 middleware 不認得任何路由。
pub(crate) fn not_in_ring(res: impl IntoResponse) -> Response {
    let mut res = res.into_response();
    res.extensions_mut().insert(NotInRing);
    res
}

/// 路由層的 middleware：讓整條路由的回應（包含 extractor 拒絕的 401／403）都不進 ring。
pub(crate) async fn not_in_ring_layer(req: Request, next: Next) -> Response {
    not_in_ring(next.run(req).await)
}

/// 這個回應是否進入 ring。
pub(crate) fn kept_in_ring(res: &Response) -> bool {
    res.extensions().get::<NotInRing>().is_none()
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
/// 因此在 `Drop` 時才寫入；傳入的 entry 其中的 `created_at`、`duration_ms`、`error_code` 會被覆寫。
pub(crate) struct StreamLog {
    store: Arc<RequestLogStore>,
    span: Span,
    started: Instant,
    /// 結束時取出補上耗時與 error code 後寫入，因此不必複製。
    entry: Option<RequestLogEntry>,
    in_ring: bool,
    outcome: SseOutcome,
}

impl StreamLog {
    pub fn new(
        store: Arc<RequestLogStore>,
        span: Span,
        started: Instant,
        entry: RequestLogEntry,
        in_ring: bool,
    ) -> Self {
        Self {
            store,
            span,
            started,
            entry: Some(entry),
            in_ring,
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
        entry.created_at = Utc::now();
        entry.duration_ms = self.started.elapsed().as_millis() as u64;
        entry.error_code = self.outcome.error_code().map(str::to_string);
        record(&self.store, &self.span, entry, self.in_ring);
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct RequestLogPage {
    #[serde(serialize_with = "serialize_shared")]
    #[schema(value_type = Vec<RequestLogEntry>)]
    items: Vec<Arc<RequestLogEntry>>,
    capacity: usize,
}

fn serialize_shared<S: serde::Serializer>(
    items: &[Arc<RequestLogEntry>],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(items.iter().map(|entry| &**entry))
}

pub fn routes() -> Router<AppState> {
    // route_layer 包住整條路由，包含 extractor 的 401／403 與方法不符的 405
    Router::new()
        .route("/api/admin/logs", get(list))
        .route_layer(middleware::from_fn(not_in_ring_layer))
}

#[utoipa::path(
    get,
    path = "/api/admin/logs",
    tag = "admin",
    summary = "查看目前程序最近的 request log",
    description = "只保留記憶體中的最近 500 筆，程序重啟即清空；不含原始 URL、header 或 body。",
    responses((status = 200, description = "最新在前的安全 request log", body = RequestLogPage), crate::error::CommonErrors)
)]
pub(crate) async fn list(_: RequireAdmin, State(state): State<AppState>) -> Json<RequestLogPage> {
    Json(RequestLogPage {
        items: state.request_logs.recent(),
        capacity: CAPACITY,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::StatusCode};
    use tower::ServiceExt;

    fn entry(n: usize) -> RequestLogEntry {
        RequestLogEntry {
            created_at: Utc::now(),
            request_id: format!("id-{n}"),
            method: "GET".into(),
            route: "/health".into(),
            status: 200,
            duration_ms: 1,
            error_code: None,
        }
    }

    #[test]
    fn reading_the_ring_shares_entries_instead_of_copying_them() {
        let store = RequestLogStore::default();
        store.push(entry(1));
        let first = store.recent();
        let second = store.recent();
        assert_eq!(first[0].request_id.as_ptr(), second[0].request_id.as_ptr());
    }

    #[test]
    fn the_ring_keeps_working_after_a_holder_panicked() {
        let store = Arc::new(RequestLogStore::default());
        let poisoner = store.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.0.lock().unwrap();
            panic!("poison the ring");
        })
        .join();
        store.push(entry(1));
        assert_eq!(store.recent().len(), 1);
    }

    #[tokio::test]
    async fn a_rejected_response_from_a_route_marked_not_in_ring_stays_out_of_the_ring() {
        let app = Router::new().route(
            "/x",
            get(|| async { StatusCode::UNAUTHORIZED })
                .layer(middleware::from_fn(not_in_ring_layer)),
        );
        let res = app
            .oneshot(Request::get("/x").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        assert!(!kept_in_ring(&res));
    }

    #[tokio::test]
    async fn an_ordinary_response_is_kept_in_the_ring() {
        let app = Router::new().route("/y", get(|| async { StatusCode::OK }));
        let res = app
            .oneshot(Request::get("/y").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert!(kept_in_ring(&res));
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
}
