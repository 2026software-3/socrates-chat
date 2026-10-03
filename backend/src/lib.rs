use std::sync::Arc;

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

use crate::{ai::AiProvider, config::Config, error::ApiError, identity::IdentityProvider};

pub mod ai;
pub mod auth;
pub mod auth_store;
pub mod catalog;
pub mod chat;
pub mod config;
pub mod error;
pub mod identity;
pub mod reply;
pub mod roster;
pub mod summary;

/// 所有 handler 共用的狀態。
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Arc<Config>,
    pub identity: Arc<dyn IdentityProvider>,
    pub ai: Arc<dyn AiProvider>,
}

impl AppState {
    pub fn new(
        pool: PgPool,
        config: Config,
        identity: Arc<dyn IdentityProvider>,
        ai: Arc<dyn AiProvider>,
    ) -> Self {
        Self {
            pool,
            config: Arc::new(config),
            identity,
            ai,
        }
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
        .merge(auth::routes())
        .merge(roster::routes())
        .merge(catalog::routes())
        .merge(chat::routes())
        .merge(reply::routes())
        .merge(summary::routes())
        .fallback(fallback)
        .layer(middleware::from_fn_with_state(state.clone(), check_origin))
        // 最外層：讓所有錯誤回應（含 CSRF 拒絕）都帶 request ID
        .layer(middleware::from_fn(request_id))
        .with_state(state)
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
