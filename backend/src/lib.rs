use std::sync::{Arc, LazyLock};

use axum::{
    Json, Router,
    extract::{Request, State},
    http::{HeaderValue, Method, header},
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
    pii::MaskingAi,
    voice::{OpenAiVoice, VoiceProvider},
};

/// SQL 條件：這場對話沒有進行中的回覆（未 claim，或 claim 已超過 `$2` 秒的逾時上限而視為過期）。
/// 送出訊息、重試與結束對話共用同一個條件，改一處即同時生效；定義在所有模組之前才能被它們使用。
macro_rules! generation_unclaimed {
    () => {
        "(generating_since IS NULL OR generating_since < now() - make_interval(secs => $2))"
    };
}

pub mod accounts;
pub mod activity_store;
pub mod ai;
pub mod auth;
pub mod auth_store;
pub mod catalog;
pub mod chat;
pub mod claims;
pub mod classroom_store;
pub mod classrooms;
pub mod client_ip;
pub mod config;
pub mod dashboard;
pub mod error;
pub mod identity;
pub mod key_pool;
pub mod login_rate_limit;
pub mod openapi;
pub mod pagination;
pub mod password;
pub mod pii;
pub mod reply;
pub mod roster;
pub mod roster_store;
pub mod summary;
pub mod voice;

/// 所有 handler 共用的狀態。
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Arc<Config>,
    pub identity: Arc<dyn IdentityProvider>,
    pub ai: Arc<dyn AiProvider>,
    /// 語音備援（S-05.3）；測試時以 `with_voice` 換成假實作。
    pub voice: Arc<dyn VoiceProvider>,
    /// 正在產生的 AI 回覆，讓離開又回到對話頁的學生接上同一個回覆。
    pub live: Arc<reply::LiveReplies>,
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
            // 所有出站 AI 請求先經過個資遮蔽（S-02.6）
            ai: Arc::new(MaskingAi::new(ai)),
            voice,
            live: Arc::default(),
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
        .merge(auth::routes(state.config.trusted_proxies.clone()))
        .merge(roster::routes())
        .merge(classrooms::routes())
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

/// 文件在執行期間不會變，只序列化一次。
async fn openapi_json() -> impl IntoResponse {
    static DOC: LazyLock<String> = LazyLock::new(|| {
        serde_json::to_string(&openapi::document()).expect("openapi document serializes")
    });
    ([(header::CONTENT_TYPE, "application/json")], DOC.as_str())
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
