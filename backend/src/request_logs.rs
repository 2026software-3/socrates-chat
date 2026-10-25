//! 管理者暫時維運頁使用的記憶體 request log；程序重啟即清空。

use std::{
    collections::VecDeque,
    sync::{Mutex, MutexGuard, PoisonError},
};

use axum::{Json, Router, extract::State, routing::get};
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::{AppState, auth::RequireAdmin};

pub const CAPACITY: usize = 500;

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

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct RequestLogPage {
    items: Vec<RequestLogEntry>,
    capacity: usize,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/admin/logs", get(list))
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
