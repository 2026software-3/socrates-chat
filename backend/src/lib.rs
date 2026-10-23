use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, MatchedPath, Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Serialize;
use sqlx::PgPool;
use tower::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};
use utoipa_redoc::{Redoc, Servable};

use crate::{
    ai::AiProvider,
    config::Config,
    error::ApiError,
    identity::IdentityProvider,
    voice::{OpenAiVoice, VoiceProvider},
};

pub mod accounts;
pub mod ai;
pub mod auth;
pub mod auth_store;
pub mod catalog;
pub mod chat;
pub mod claims;
pub mod config;
pub mod dashboard;
pub mod error;
pub mod identity;
pub mod openapi;
pub mod pagination;
pub mod password;
pub mod reply;
pub mod roster;
pub mod summary;
pub mod voice;

/// 全域請求 body 上限（#324）。最大的合法 JSON body 是修課名單匯入（每行一個信箱，
/// 約 40 bytes／行；1 MiB 可容納超過兩萬筆）；訊息上限 4000 字元，UTF-8 加上 JSON 跳脫也不到 30 KB。
/// 語音辨識上傳音訊，另有自己的上限（`voice::MAX_AUDIO_BYTES`）。
pub const MAX_BODY_BYTES: usize = 1024 * 1024;

/// 不套用一般請求逾時、改用 `Config::long_request_timeout` 的路由（#325）。
/// 逾時只計到送出回應標頭為止。SSE 路由（送出訊息、重試串流）不在此列：AI 回覆在背景工作產生，
/// 標頭送出後串流本身不受逾時影響（長度由 AI 逾時限制，S-03.5），標頭之前只有讀取 JSON body 與資料庫查詢。
const LONG_RUNNING_ROUTES: [&str; 4] = [
    // 同步等 AI 重新分組
    "/api/dashboard/class/regroup",
    // 上傳最多 10 MB 音訊，並同步等語音服務
    "/api/voice/transcribe",
    // 同步等語音服務合成語音
    "/api/voice/speech",
    // 管理者匯入時逐一雜湊新帳號的臨時密碼（Argon2），名單大時會超過一般逾時
    "/api/roster/import",
];

/// 所有 handler 共用的狀態。
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Arc<Config>,
    pub identity: Arc<dyn IdentityProvider>,
    pub ai: Arc<dyn AiProvider>,
    /// 語音備援（S-05.3）；測試時以 `with_voice` 換成假實作。
    pub voice: Arc<dyn VoiceProvider>,
}

impl AppState {
    pub fn new(
        pool: PgPool,
        config: Config,
        identity: Arc<dyn IdentityProvider>,
        ai: Arc<dyn AiProvider>,
    ) -> Self {
        let voice = Arc::new(OpenAiVoice::from_config(&config));
        Self {
            pool,
            config: Arc::new(config),
            identity,
            ai,
            voice,
        }
    }

    pub fn with_voice(mut self, voice: Arc<dyn VoiceProvider>) -> Self {
        self.voice = voice;
        self
    }
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
}

/// Builds the application router.
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/openapi.json", get(openapi_json))
        .merge(auth::routes())
        .merge(roster::routes())
        .merge(accounts::routes())
        .merge(dashboard::routes())
        .merge(claims::routes())
        .merge(catalog::routes())
        .merge(chat::routes())
        .merge(reply::routes())
        .merge(summary::routes())
        .merge(voice::routes())
        // API reference：由 OpenAPI 文件渲染的 Redoc 頁面，公開；Redoc 的 JS 由後端自己提供
        .route("/api/docs/redoc.standalone.js", get(redoc_js))
        .merge(
            Redoc::with_url("/api/docs", openapi::document())
                .custom_html(include_str!("redoc.html")),
        )
        .fallback(fallback)
        .layer(middleware::from_fn_with_state(state.clone(), check_origin))
        // 明確設定全域 body 上限；個別路由（語音辨識）可在自己的 router 內放寬
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn(payload_too_large))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            request_timeout,
        ))
        // 最外層：讓所有錯誤回應（含 CSRF 拒絕）都帶 request ID
        .layer(middleware::from_fn(request_id))
        .with_state(state)
}

/// Redoc 2.5.4 的 bundle（MIT，來源與授權見 `src/redoc/` 與 docs/THIRD-PARTY-NOTICES.md），
/// 由後端自己提供，讓 API reference 不依賴外部 CDN。
async fn redoc_js() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_str!("redoc/redoc.standalone.js"),
    )
}

async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(openapi::document())
}

async fn health() -> Json<Health> {
    Json(Health { status: "ok" })
}

/// 沒有對應路由時：`/api` 底下一律回 JSON 404；其餘在設定了 `FRONTEND_DIR` 時提供前端建置產物，
/// 找不到的檔案退回 `index.html`（前端路由，S-08.1）。
async fn fallback(State(state): State<AppState>, req: Request) -> Response {
    let path = req.uri().path();
    let is_api = path == "/api" || path.starts_with("/api/");
    let Some(dir) = state.config.frontend_dir.as_ref().filter(|_| !is_api) else {
        return ApiError::not_found().into_response();
    };
    let spa = ServeDir::new(dir).fallback(ServeFile::new(dir.join("index.html")));
    match spa.oneshot(req).await {
        Ok(res) => res.into_response(),
        Err(never) => match never {},
    }
}

/// 為每個請求產生 request ID：放進錯誤回應與 `x-request-id` 標頭。
async fn request_id(req: Request, next: Next) -> Response {
    let id = uuid::Uuid::new_v4().to_string();
    let mut res = error::REQUEST_ID.scope(id.clone(), next.run(req)).await;
    if let Ok(v) = HeaderValue::from_str(&id) {
        res.headers_mut().insert("x-request-id", v);
    }
    res
}

/// body 超過上限時，axum 的 extractor 回 413 純文字；改成統一的 JSON 錯誤格式（S-08.1）。
/// 不論有沒有 `Content-Length`（分段傳送讀到超過上限才發現），都是同一個 413。
async fn payload_too_large(req: Request, next: Next) -> Response {
    let res = next.run(req).await;
    if res.status() == StatusCode::PAYLOAD_TOO_LARGE {
        return ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "請求內容過大",
        )
        .into_response();
    }
    res
}

/// 請求逾時（#325）：從收到請求到送出回應標頭為止（含讀取 body 與 handler 處理），
/// 避免慢速送出 body 的連線（slow-loris）長時間占用。逾時時中止 handler（交易未提交即回滾），
/// 回 `408 request_timeout`。
async fn request_timeout(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let long = req
        .extensions()
        .get::<MatchedPath>()
        .is_some_and(|p| LONG_RUNNING_ROUTES.contains(&p.as_str()));
    let limit = if long {
        state.config.long_request_timeout()
    } else {
        state.config.request_timeout
    };
    match tokio::time::timeout(limit, next.run(req)).await {
        Ok(res) => res,
        Err(_) => ApiError::new(StatusCode::REQUEST_TIMEOUT, "request_timeout", "請求逾時")
            .into_response(),
    }
}

/// CSRF 防護（S-08.2）：會改變狀態的請求，`Origin` 必須等於本站網域。
async fn check_origin(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let safe = matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if !safe {
        let origin = req
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok());
        if origin != Some(state.config.app_base_url.as_str()) {
            return Err(ApiError::forbidden("csrf", "請求來源不被允許"));
        }
    }
    Ok(next.run(req).await)
}
