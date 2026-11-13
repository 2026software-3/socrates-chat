use std::{
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
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
    error::{ApiError, ErrorBody},
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

mod access_log;
pub mod accounts;
pub mod achievement_admin;
pub mod achievements;
pub mod activity_store;
pub mod ai;
pub mod ai_usage;
pub mod announcements;
pub mod argument_tree;
pub mod auth;
pub mod auth_store;
pub mod cascade;
pub mod catalog;
pub mod chat;
pub mod claims;
pub mod classroom_store;
pub mod classrooms;
pub mod client_ip;
pub mod complaints;
pub mod config;
pub mod dashboard;
pub mod demo;
pub mod diagnostics;
pub mod error;
mod google_translation;
pub mod identity;
pub mod key_pool;
pub mod logging;
pub mod login_rate_limit;
pub mod mail;
pub mod main_argument_tree;
pub(crate) mod mutex;
pub mod openapi;
pub mod pagination;
pub mod password;
pub mod pii;
pub mod points;
pub mod purchase;
pub mod reply;
pub mod roster;
pub mod roster_store;
pub mod summary;
pub mod summary_translation;
pub mod tasks;
pub(crate) mod user_limit;
pub mod voice;

/// 全域請求 body 上限（#324）。最大的合法 JSON body 是修課名單匯入（每行一個信箱，
/// 約 40 bytes／行；1 MiB 可容納超過兩萬筆）；訊息上限 4000 字元，UTF-8 加上 JSON 跳脫也不到 30 KB。
/// 語音辨識上傳音訊，另有自己的上限（`voice::MAX_AUDIO_BYTES`）。
pub const MAX_BODY_BYTES: usize = 1024 * 1024;

/// 改用 `Config::long_request_timeout` 的路由（#325）。SSE 路由不在此列：AI 回覆在背景工作產生，
/// 標頭之前只有讀取 JSON body 與資料庫查詢，標頭送出後串流不受逾時影響（S-03.5）。
pub const LONG_RUNNING_ROUTES: [&str; 8] = [
    // 管理者新增成就時同步翻譯名稱與說明
    "/api/admin/achievements",
    // 管理者新增、修改公告時同步翻譯內文
    "/api/admin/announcements",
    "/api/admin/announcements/{id}",
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
    pub(crate) google_translation: Option<google_translation::GoogleTranslation>,
    /// 語音備援（S-05.3）；測試時以 `with_voice` 換成假實作。
    pub voice: Arc<dyn VoiceProvider>,
    /// 自架語音同時進行中的使用者（S-05.5）。
    pub(crate) speech_slots: Arc<voice::SpeechSlots>,
    /// 進行中的語音對話（實驗，見 `cascade`）。
    pub voice_sessions: Arc<cascade::VoiceSessions>,
    /// 正在產生的 AI 回覆，讓離開又回到對話頁的學生接上同一個回覆。
    pub live: Arc<reply::LiveReplies>,
    /// 教師按需翻譯總結的每人頻率限制。
    pub(crate) translation_limit: Arc<user_limit::UserLimiter>,
    pub(crate) summary_translation_wake: Arc<tokio::sync::Notify>,
    /// 每位使用者的 AI 用量上限（`ai_usage`）；放在記憶體，重啟後歸零，多個實例各自計算。
    pub(crate) ai_usage: Arc<user_limit::UserLimiter>,
    /// 維護模式旗標的快取（S-15.2）；真相在資料庫。
    pub(crate) maintenance: Arc<announcements::MaintenanceFlag>,
}

impl AppState {
    pub fn new(
        pool: PgPool,
        config: Config,
        identity: Arc<dyn IdentityProvider>,
        ai: Arc<dyn AiProvider>,
    ) -> Self {
        let voice = Arc::new(OpenAiVoice::from_config(&config));
        let google_translation = config
            .google_translation_api_key
            .as_deref()
            .and_then(google_translation::GoogleTranslation::new);
        let ai_usage = Arc::new(user_limit::UserLimiter::new(
            config.ai_usage_max,
            config.ai_usage_window,
        ));
        Self {
            pool,
            config: Arc::new(config),
            identity,
            // 所有出站 AI 請求先經過個資遮蔽（S-02.6）
            ai: Arc::new(MaskingAi::new(ai)),
            google_translation,
            voice,
            speech_slots: Arc::default(),
            voice_sessions: Arc::default(),
            live: Arc::default(),
            translation_limit: Arc::default(),
            summary_translation_wake: Arc::default(),
            ai_usage,
            maintenance: Arc::default(),
        }
    }

    pub fn with_voice(mut self, voice: Arc<dyn VoiceProvider>) -> Self {
        self.voice = voice;
        self
    }

    pub fn with_google_translation(mut self, key: &str) -> Self {
        self.google_translation = google_translation::GoogleTranslation::new(key);
        self
    }

    /// 換掉教師翻譯的頻率限制（測試用）。
    pub fn with_translation_limit(mut self, max: usize, window: std::time::Duration) -> Self {
        self.translation_limit = Arc::new(user_limit::UserLimiter::new(max, window));
        self
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct Health {
    #[schema(example = "ok")]
    status: &'static str,
}

/// 就緒檢查等資料庫回應的時間上限。資料庫卡住（連線被占滿、網路黑洞）時探針不能跟著卡住：
/// 它比 pool 預設的取連線逾時（30 秒）與全域請求逾時（10 秒）都短。
/// 探針端的逾時設定必須大於這個值，才收得到結構化的 503 而不是探針自己逾時。
/// 規格（S-09.3）沒有指定秒數，3 秒是保守的假設。
const READY_DB_TIMEOUT: Duration = Duration::from_secs(3);

/// Builds the application router.
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/api/openapi.json", get(openapi_json))
        .merge(auth::routes(
            state.config.trusted_proxies.clone(),
            state.config.login_rate_burst,
            state.config.login_rate_replenish,
        ))
        .merge(roster::routes())
        .merge(classrooms::routes())
        .merge(accounts::routes())
        .merge(dashboard::routes())
        .merge(claims::routes())
        .merge(catalog::routes())
        .merge(chat::routes())
        .merge(reply::routes())
        .merge(diagnostics::routes())
        .merge(summary::routes())
        .merge(argument_tree::routes())
        .merge(voice::routes())
        .merge(cascade::routes())
        .merge(points::routes())
        .merge(tasks::routes())
        .merge(purchase::routes())
        .merge(achievement_admin::routes())
        .merge(complaints::routes())
        .merge(demo::routes())
        .merge(ai_usage::routes())
        .merge(announcements::routes())
        // API reference：由 OpenAPI 文件渲染的 Redoc 頁面，公開；Redoc 的 JS 由後端自己提供
        .route("/api/docs/redoc.standalone.js", get(redoc_js))
        .merge(
            Redoc::with_url("/api/docs", openapi::document())
                .custom_html(include_str!("redoc.html")),
        )
        .fallback(fallback)
        // 最內層：維護模式開啟時擋下非管理者（S-15.2）；其餘層（CSRF、逾時、request ID、安全標頭）照常包住它
        .layer(middleware::from_fn_with_state(
            state.clone(),
            maintenance_gate,
        ))
        .layer(middleware::from_fn(api_rejections))
        .layer(middleware::from_fn_with_state(state.clone(), check_origin))
        // 明確設定全域 body 上限；語音辨識在自己的 router 內放寬。超過時的 413 由 `api_rejections` 轉成 JSON
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            request_timeout,
        ))
        // 最外層：讓所有錯誤回應（含 CSRF 拒絕）都帶 request ID
        .layer(middleware::from_fn(request_id))
        // 真正的最外層：所有回應（含錯誤與 CSRF 拒絕）都補上安全標頭
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security_headers,
        ))
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

/// 就緒檢查（S-09.3）：以 `SELECT 1` 確認資料庫連得上，失敗或逾時回 503。
/// 與 `/health`（只表示程序存活）不同；探針不需登入。日誌與回應只帶固定的錯誤種類，
/// 不含連線字串、主機或資料庫的錯誤訊息。失敗的紀錄看 stdout 的 tracing 輸出。
#[utoipa::path(
    get,
    path = "/ready",
    tag = "ops",
    summary = "就緒檢查",
    description = "檢查資料庫連線（`SELECT 1`，最多等 3 秒）。公開，供探針與告警使用；`/health` 則只表示程序存活。",
    security(()),
    responses(
        (status = 200, description = "資料庫可連線", body = Health),
        (status = 503, description = "資料庫無法連線或逾時（`database_unavailable`）", body = ErrorBody)
    )
)]
pub(crate) async fn ready(State(state): State<AppState>) -> Response {
    let check = tokio::time::timeout(
        READY_DB_TIMEOUT,
        sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&state.pool),
    )
    .await;
    match check {
        Ok(Ok(_)) => Json(Health { status: "ok" }).into_response(),
        Ok(Err(e)) => {
            tracing::warn!(class = %error::db_error_class(&e), "readiness check failed");
            database_unavailable()
        }
        Err(_) => {
            tracing::warn!(class = "timeout", "readiness check failed");
            database_unavailable()
        }
    }
}

fn database_unavailable() -> Response {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "database_unavailable",
        "資料庫暫時無法使用",
    )
    .into_response()
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
async fn request_id(req: Request, next: Next) -> Response {
    let request_uuid = uuid::Uuid::new_v4();
    let id = request_uuid.to_string();
    let method = req.method().clone();
    // 只記錄路由模板；原始 URL 可能含 email 等個資。
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str)
        .unwrap_or(access_log::FALLBACK_ROUTE)
        .to_string();
    let span = tracing::info_span!(
        "http_request",
        request_id = %id,
        method = %method,
        route = %route
    );
    let started = Instant::now();
    let mut res = error::REQUEST_ID
        .scope(
            id.clone(),
            diagnostics::scope_request(
                request_uuid,
                &route,
                next.run(req).instrument(span.clone()),
            ),
        )
        .await;
    if let Ok(v) = HeaderValue::from_str(&id) {
        res.headers_mut().insert("x-request-id", v);
    }
    let status = res.status().as_u16();
    let error_code = res
        .extensions()
        .get::<error::ErrorCode>()
        .map(|code| code.0.to_string());
    let entry = access_log::RequestLogEntry {
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
        let mut log = access_log::StreamLog::new(span, started, entry);
        let body = Body::from_stream(body.into_data_stream().map(move |chunk| {
            if let Ok(bytes) = &chunk {
                log.observe(bytes);
            }
            chunk
        }));
        return Response::from_parts(parts, body);
    }
    access_log::record(&span, &entry);
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

/// 維護模式中照常服務的路徑（S-15.2）：公告端點（登入頁據此顯示維護提示）、登入與登出
/// （非管理者的登入在核發 session 前另行拒絕）、API 文件。`/health`、`/ready` 與前端檔案不在 `/api` 底下。
fn maintenance_exempt(path: &str) -> bool {
    matches!(
        path,
        "/api/announcements"
            | "/api/openapi.json"
            | "/api/auth/login"
            | "/api/auth/logout"
            | "/api/auth/google/login"
            | "/api/auth/google/callback"
    ) || path.starts_with("/api/docs")
}

/// 維護模式（S-15.2）：開啟時，`/api` 與 cascade 內部端點（`/internal`）只放行管理者與例外路徑，
/// 其餘回 503 `maintenance`。已經開始的串流（進行中的 AI 回覆）不受影響，因為只檢查請求開始的當下。
async fn maintenance_gate(State(state): State<AppState>, req: Request, next: Next) -> Response {
    use axum::extract::FromRequestParts;

    let path = req.uri().path();
    let gated = is_api_path(path) || path.starts_with("/internal/");
    if !gated || maintenance_exempt(path) || !state.maintenance.is_on(&state.pool).await {
        return next.run(req).await;
    }
    let (mut parts, body) = req.into_parts();
    let is_admin = auth::AnyCurrentUser::from_request_parts(&mut parts, &state)
        .await
        .is_ok_and(|auth::AnyCurrentUser(cu)| cu.roles.admin);
    if is_admin {
        next.run(Request::from_parts(parts, body)).await
    } else {
        announcements::MAINTENANCE_ERROR.into_response()
    }
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
    // 內部端點只認共用密鑰、不看 cookie，瀏覽器帶不上身分，沒有 CSRF 問題
    if !safe && !req.uri().path().starts_with(cascade::INTERNAL_PREFIX) {
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

/// 一般頁面（SPA 與 API）的 CSP：只允許同源資源。樣式需 `'unsafe-inline'`（React 的 `style` 屬性）；
/// `blob:` 給語音播放與圖片預覽。
const CSP_DEFAULT: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
img-src 'self' data: blob:; media-src 'self' blob:; font-src 'self' data:; connect-src 'self'; \
object-src 'none'; base-uri 'self'; frame-ancestors 'none'";

/// `/api/docs` 的 Redoc 頁面有行內初始化 script，且用 blob worker，只有這個路徑放寬。
const CSP_API_DOCS: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; \
style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; \
connect-src 'self'; worker-src blob:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'";

/// HSTS 一年。不加 `includeSubDomains`／`preload`：網域可能是共用的（例如 Tailscale 的 ts.net）。
const HSTS: &str = "max-age=31536000";

/// 安全回應標頭：CSP、`X-Frame-Options`、`nosniff`、`Referrer-Policy`；
/// 只有 `APP_BASE_URL` 是 https 時才送 HSTS（http 開發環境送了會讓瀏覽器之後拒絕 http）。
async fn security_headers(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let path = req.uri().path();
    let csp = if path == "/api/docs" || path.starts_with("/api/docs/") {
        CSP_API_DOCS
    } else {
        CSP_DEFAULT
    };
    let mut res = next.run(req).await;
    let headers = res.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(csp),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    if state.config.app_base_url.starts_with("https://") {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static(HSTS),
        );
    }
    res
}
