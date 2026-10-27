use std::{
    sync::{Arc, LazyLock},
    time::Instant,
};

use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, MatchedPath, Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::StreamExt;
use serde::Serialize;
use sqlx::PgPool;
use tower::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};
use tracing::Instrument;
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
pub mod logging;
pub mod login_rate_limit;
pub(crate) mod mutex;
pub mod openapi;
pub mod pagination;
pub mod password;
pub mod pii;
pub mod points;
pub mod reply;
pub mod request_logs;
pub mod roster;
pub mod roster_store;
pub mod summary;
pub(crate) mod translation_limit;
pub mod voice;

/// 全域請求 body 上限（#324）。最大的合法 JSON body 是修課名單匯入（每行一個信箱，
/// 約 40 bytes／行；1 MiB 可容納超過兩萬筆）；訊息上限 4000 字元，UTF-8 加上 JSON 跳脫也不到 30 KB。
/// 語音辨識上傳音訊，另有自己的上限（`voice::MAX_AUDIO_BYTES`）。
pub const MAX_BODY_BYTES: usize = 1024 * 1024;

/// 改用 `Config::long_request_timeout` 的路由（#325）。SSE 路由不在此列：AI 回覆在背景工作產生，
/// 標頭之前只有讀取 JSON body 與資料庫查詢，標頭送出後串流不受逾時影響（S-03.5）。
pub const LONG_RUNNING_ROUTES: [&str; 5] = [
    // 同步等 AI 重新分組
    "/api/dashboard/class/regroup",
    // 同步等 AI 翻譯總結；推理模型常比一般逾時慢得多
    "/api/summaries/{id}/translation",
    // 上傳最多 10 MB 音訊，並同步等語音服務
    "/api/voice/transcribe",
    // 同步等語音服務合成語音
    "/api/voice/speech",
    // 管理者匯入時逐一雜湊新帳號的臨時密碼（Argon2），名單大時會超過一般逾時
    "/api/classrooms/{id}/roster/import",
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
    /// 正在產生的 AI 回覆，讓離開又回到對話頁的學生接上同一個回覆。
    pub live: Arc<reply::LiveReplies>,
    /// 管理者暫時維運頁使用的近期 request log；程序重啟即清空。
    pub(crate) request_logs: Arc<request_logs::RequestLogStore>,
    /// 教師按需翻譯總結的每人頻率限制（每次都會呼叫 AI 且不快取）。
    pub(crate) translation_limit: Arc<translation_limit::TranslationLimiter>,
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
            request_logs: Arc::default(),
            translation_limit: Arc::default(),
        }
    }

    pub fn with_voice(mut self, voice: Arc<dyn VoiceProvider>) -> Self {
        self.voice = voice;
        self
    }

    /// 換掉教師翻譯的頻率限制（測試用）。
    pub fn with_translation_limit(mut self, max: usize, window: std::time::Duration) -> Self {
        self.translation_limit = Arc::new(translation_limit::TranslationLimiter::new(max, window));
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
        .merge(request_logs::routes())
        .merge(summary::routes())
        .merge(voice::routes())
        .merge(points::routes())
        // API reference：由 OpenAPI 文件渲染的 Redoc 頁面，公開；Redoc 的 JS 由後端自己提供
        .route("/api/docs/redoc.standalone.js", get(redoc_js))
        .merge(
            Redoc::with_url("/api/docs", openapi::document())
                .custom_html(include_str!("redoc.html")),
        )
        .fallback(fallback)
        .layer(middleware::from_fn(api_rejections))
        .layer(middleware::from_fn_with_state(state.clone(), check_origin))
        // 明確設定全域 body 上限；語音辨識在自己的 router 內放寬。超過時的 413 由 `api_rejections` 轉成 JSON
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            request_timeout,
        ))
        // 最外層：讓所有錯誤回應（含 CSRF 拒絕）都帶 request ID
        .layer(middleware::from_fn_with_state(state.clone(), request_id))
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

/// 健康檢查會被外部探測器頻繁呼叫，不進 request log ring（stdout 仍輸出）。
async fn health() -> Response {
    request_logs::not_in_ring(Json(Health { status: "ok" }))
}

/// 沒有對應路由時：`/api` 底下一律回 JSON 404；其餘在設定了 `FRONTEND_DIR` 時提供前端建置產物，
/// 找不到的檔案退回 `index.html`（前端路由，S-08.1）。
async fn fallback(State(state): State<AppState>, req: Request) -> Response {
    let path = req.uri().path();
    let is_api = is_api_path(path);
    let Some(dir) = state.config.frontend_dir.as_ref().filter(|_| !is_api) else {
        return ApiError::not_found().into_response();
    };
    let spa = ServeDir::new(dir).fallback(ServeFile::new(dir.join("index.html")));
    match spa.oneshot(req).await {
        Ok(res) => res.into_response(),
        Err(never) => match never {},
    }
}

/// 為每個請求產生 request ID，並以不含原始 URL 或本文的結構化欄位記錄結果。
async fn request_id(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let id = uuid::Uuid::new_v4().to_string();
    let method = req.method().clone();
    // 只記錄路由模板；原始 URL 可能含 email 等個資。
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str)
        .unwrap_or(request_logs::FALLBACK_ROUTE)
        .to_string();
    let span = tracing::info_span!(
        "http_request",
        request_id = %id,
        method = %method,
        route = %route
    );
    let started = Instant::now();
    let mut res = error::REQUEST_ID
        .scope(id.clone(), next.run(req).instrument(span.clone()))
        .await;
    if let Ok(v) = HeaderValue::from_str(&id) {
        res.headers_mut().insert("x-request-id", v);
    }
    let status = res.status().as_u16();
    let error_code = res
        .extensions()
        .get::<error::ErrorCode>()
        .map(|code| code.0.to_string());
    // 沒有對應路由的請求（掃描、前端靜態檔、被 CSRF 擋下的未知路徑）會擠掉真正的應用請求，不進 ring；
    // 其餘由路由或處理函式自己標記。stdout 的 tracing event 一律輸出。
    let in_ring = route != request_logs::FALLBACK_ROUTE && request_logs::kept_in_ring(&res);
    let entry = request_logs::RequestLogEntry {
        created_at: chrono::Utc::now(),
        request_id: id,
        method: method.to_string(),
        route,
        status,
        duration_ms: started.elapsed().as_millis() as u64,
        error_code,
    };
    if is_event_stream(&res) {
        // 串流的結果（最後的 done／error 事件）要等本文讀完才知道，因此記錄延後到本文結束或被丟棄時。
        let (parts, body) = res.into_parts();
        let mut log =
            request_logs::StreamLog::new(state.request_logs.clone(), span, started, entry, in_ring);
        let body = Body::from_stream(body.into_data_stream().map(move |chunk| {
            if let Ok(bytes) = &chunk {
                log.observe(bytes);
            }
            chunk
        }));
        return Response::from_parts(parts, body);
    }
    request_logs::record(&state.request_logs, &span, entry, in_ring);
    res
}

fn is_event_stream(res: &Response) -> bool {
    res.headers()
        .get(header::CONTENT_TYPE)
        .is_some_and(|v| v.as_bytes().starts_with(b"text/event-stream"))
}

fn is_api_path(path: &str) -> bool {
    path == "/api" || path.starts_with("/api/")
}

/// 框架本身的拒絕（`Json`／`Path`／`Query` 解析失敗、方法不符、本文過大）是純文字，沒有 API error code。
/// `/api` 底下統一轉成標準錯誤格式，讓前端與 request log 都拿得到固定的 code；
/// 原回應的標頭（例如 405 的 `Allow`）與 extension（例如不進 ring 的標記）都保留。
async fn api_rejections(req: Request, next: Next) -> Response {
    let is_api = is_api_path(req.uri().path());
    let res = next.run(req).await;
    if !is_api || res.extensions().get::<error::ErrorCode>().is_some() {
        return res;
    }
    let (code, message) = match res.status() {
        StatusCode::BAD_REQUEST => ("bad_request", "請求格式不正確"),
        StatusCode::METHOD_NOT_ALLOWED => ("method_not_allowed", "不支援此方法"),
        StatusCode::PAYLOAD_TOO_LARGE => ("payload_too_large", "請求內容過大"),
        StatusCode::UNSUPPORTED_MEDIA_TYPE => ("unsupported_media_type", "不支援的內容類型"),
        StatusCode::UNPROCESSABLE_ENTITY => ("invalid_body", "請求內容不符合格式"),
        _ => return res,
    };
    let (original, _) = res.into_parts();
    let mut replaced = ApiError::new(original.status, code, message).into_response();
    for (name, value) in &original.headers {
        if name != header::CONTENT_TYPE && name != header::CONTENT_LENGTH {
            replaced.headers_mut().append(name.clone(), value.clone());
        }
    }
    replaced.extensions_mut().extend(original.extensions);
    replaced
}

/// 請求逾時（#325）：從收到請求到送出回應標頭為止（含讀取 body 與 handler 處理），
/// 避免慢速送出 body 的連線（slow-loris）長時間占用。逾時時中止 handler（交易未提交即回滾），
/// 回 `408 request_timeout`。
async fn request_timeout(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let limit = if req
        .extensions()
        .get::<MatchedPath>()
        .is_some_and(|p| LONG_RUNNING_ROUTES.contains(&p.as_str()))
    {
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
