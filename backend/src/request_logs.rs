//! 管理者暫時維運頁使用的記憶體 request log；程序重啟即清空。

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
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

use crate::{AppState, auth::RequireAdmin};

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

#[derive(Default)]
pub(crate) struct RequestLogStore(Mutex<VecDeque<RequestLogEntry>>);

impl RequestLogStore {
    pub fn push(&self, entry: RequestLogEntry) {
        let mut entries = lock(&self.0);
        if entries.len() == CAPACITY {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    fn recent(&self) -> Vec<RequestLogEntry> {
        lock(&self.0).iter().rev().cloned().collect()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
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
            error_code = entry.error_code.as_deref().unwrap_or(""),
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

/// 串流回應的結果：本文中最後一個 `error` 事件的 code。`done` 與其他事件沒有 error code。
#[derive(Default)]
pub(crate) struct SseOutcome {
    /// 尚未收滿一個事件（以空行結束）的位元組；事件可能跨越多個片段。
    pending: Vec<u8>,
    error_code: Option<String>,
}

impl SseOutcome {
    pub fn observe(&mut self, chunk: &[u8]) {
        self.pending.extend_from_slice(chunk);
        while let Some(end) = self.pending.windows(2).position(|w| w == b"\n\n") {
            let event: Vec<u8> = self.pending.drain(..end + 2).collect();
            self.observe_event(&String::from_utf8_lossy(&event));
        }
    }

    pub fn error_code(&self) -> Option<&str> {
        self.error_code.as_deref()
    }

    fn observe_event(&mut self, event: &str) {
        let mut name = None;
        let mut data = None;
        for line in event.lines() {
            if let Some(v) = line.strip_prefix("event:") {
                name = Some(v.trim());
            } else if let Some(v) = line.strip_prefix("data:") {
                data = Some(v.trim());
            }
        }
        if name == Some("error") {
            self.error_code = Some(
                data.and_then(|d| serde_json::from_str::<serde_json::Value>(d).ok())
                    .and_then(|v| v["code"].as_str().map(str::to_string))
                    .unwrap_or_else(|| "error".to_string()),
            );
        }
    }
}

/// 串流回應的 request log：耗時與 error code 只有在本文結束（或連線中斷、本文被丟棄）時才確定，
/// 因此在 `Drop` 時才寫入；傳入的 entry 其中的 `created_at`、`duration_ms`、`error_code` 會被覆寫。
pub(crate) struct StreamLog {
    store: Arc<RequestLogStore>,
    span: Span,
    started: Instant,
    entry: RequestLogEntry,
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
            entry,
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
        let entry = RequestLogEntry {
            created_at: Utc::now(),
            duration_ms: self.started.elapsed().as_millis() as u64,
            error_code: self.outcome.error_code().map(str::to_string),
            ..self.entry.clone()
        };
        record(&self.store, &self.span, entry, self.in_ring);
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct RequestLogPage {
    items: Vec<RequestLogEntry>,
    capacity: usize,
}

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/admin/logs",
        get(list).layer(middleware::from_fn(not_in_ring_layer)),
    )
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
    fn stream_without_an_error_event_has_no_error_code() {
        let mut outcome = SseOutcome::default();
        outcome.observe(b": keep-alive\n\n");
        outcome.observe(b"event: done\ndata: {}\n\n");
        assert_eq!(outcome.error_code(), None);
    }
}
