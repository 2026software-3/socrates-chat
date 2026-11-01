//! 每位使用者的 AI 用量上限，以及 `GET /api/me/usage`（前端顯示用量）。
//!
//! 沒有上限的話，任何登入者（包含公開的示範帳號）都能讓 AI 費用無上限。只計入使用者按下去才發生的呼叫：
//! 對話回覆（含重試）、語音辨識與合成、教師翻譯總結與重新分組；對話結束後自動產生的總結與分組不計，
//! 因為它們受回合上限約束。呼叫失敗（沒有得到結果）會退回額度。
//! 計數放在記憶體：重啟後歸零，多個實例各自計算（部署方式尚未決定，S-09）。
//! 預設值與時間窗是暫定值，待需求方確認。

use std::time::{Duration, Instant};

use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use serde::Serialize;
use uuid::Uuid;

use crate::{AppState, auth::AnyCurrentUser, error::ApiError};

/// 每位使用者在一個時間窗內最多可觸發幾次 AI 呼叫。一場對話最多 `MAX_TURNS`（預設 20）回合，
/// 這個額度夠一個人一天進行多場對話與語音互動。暫定值，待需求方確認。
pub const DEFAULT_MAX: usize = 200;
/// 計算額度的滑動時間窗：24 小時。暫定值，待需求方確認。
pub const DEFAULT_WINDOW: Duration = Duration::from_secs(86_400);

/// 錯誤碼：HTTP 回應與 SSE 的 `error` 事件共用。
pub const EXCEEDED_CODE: &str = "ai_usage_exceeded";

pub(crate) const EXCEEDED: ApiError = ApiError::new(
    StatusCode::TOO_MANY_REQUESTS,
    EXCEEDED_CODE,
    "AI 用量已達上限，請稍後再試",
);

impl AppState {
    /// 為這位使用者的一次 AI 呼叫扣一個額度；用完回傳 429 `ai_usage_exceeded`。
    /// 回傳的記錄在呼叫沒有得到結果時交給 `refund_ai`。
    pub(crate) fn charge_ai(&self, user: Uuid) -> Result<Instant, ApiError> {
        self.ai_usage.try_acquire(user).ok_or(EXCEEDED)
    }

    /// 退回 `charge_ai` 的那一筆額度。
    pub(crate) fn refund_ai(&self, user: Uuid, slot: Instant) {
        self.ai_usage.refund(user, slot);
    }
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/me/usage", get(usage))
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct Usage {
    /// 目前時間窗內已用的 AI 呼叫次數。
    used: usize,
    /// 時間窗內的上限。
    limit: usize,
    /// 滑動時間窗的長度（秒）。
    window_secs: u64,
    /// 最早那一次呼叫還要幾秒才滑出時間窗（也就是多久後會空出一個額度）；尚未使用時為 `null`。
    resets_in_secs: Option<u64>,
}

/// 目前登入者自己的 AI 用量。
#[utoipa::path(
    get,
    path = "/api/me/usage",
    tag = "auth",
    summary = "自己的 AI 用量",
    description = "滑動時間窗內已用的 AI 呼叫次數與上限。用完時，對話回覆的 SSE 會以 error 事件 `ai_usage_exceeded` 結束，語音與教師功能回 429 `ai_usage_exceeded`。",
    responses((status = 200, description = "用量", body = Usage), crate::error::CommonErrors)
)]
pub(crate) async fn usage(
    AnyCurrentUser(cu): AnyCurrentUser,
    State(state): State<AppState>,
) -> Json<Usage> {
    let s = state.ai_usage.snapshot(cu.user.id);
    Json(Usage {
        used: s.used,
        limit: s.max,
        window_secs: s.window.as_secs(),
        // 無條件進位到整秒，避免剩不到 1 秒時顯示 0 而讓人以為已經重置
        resets_in_secs: s
            .resets_in
            .map(|d| d.as_secs() + u64::from(d.subsec_nanos() > 0)),
    })
}
