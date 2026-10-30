//! 登入、登出與 session 驗證（F-01.1，規格見 S-08.2）。

use std::net::SocketAddr;

use axum::{
    Extension, Json, Router,
    extract::{ConnectInfo, FromRequestParts, Query, State},
    http::{HeaderMap, StatusCode, header, request::Parts},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    AppState,
    accounts::audit,
    auth_store,
    auth_store::{SESSION_ABSOLUTE_TTL, SessionUser},
    catalog::nullable,
    client_ip,
    error::ApiError,
    login_rate_limit, password,
};

const COOKIE_NAME: &str = "sid";

/// 帳號已被管理者停用（S-01.3）：登入後看到「帳號已停用」。
const ACCOUNT_DISABLED: ApiError = ApiError::forbidden("account_disabled", "帳號已停用");

pub fn routes(trusted_proxies: Vec<client_ip::Cidr>) -> Router<AppState> {
    Router::new()
        .route("/api/auth/google/login", get(login))
        .route("/api/auth/google/callback", get(callback))
        .route(
            "/api/auth/login",
            login_rate_limit::apply(post(password_login), trusted_proxies),
        )
        .route("/api/auth/change-password", post(change_password))
        .route("/api/admin/users/reset-password", post(reset_password))
        .route("/api/auth/logout", post(logout))
        .route("/api/me", get(me).patch(update_me))
}

/// 256 位元隨機 token（base64url）。
fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// 資料庫只存 token 的 SHA-256 雜湊。
pub fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn session_cookie(state: &AppState, token: &str, max_age_secs: i64) -> String {
    let secure = if state.config.cookie_secure {
        "; Secure"
    } else {
        ""
    };
    format!("{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}{secure}")
}

fn token_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE_NAME)
        .map(|(_, v)| v)
        .filter(|v| !v.is_empty())
}

/// `Accept-Language` 中權重（q）最高的支援語言，同分取先出現的；`q=0` 代表瀏覽器不接受，
/// 一律略過。沒有可接受的支援語言時使用繁體中文。
fn preferred_language(headers: &HeaderMap) -> &'static str {
    let mut best: Option<(&'static str, f32)> = None;
    let value = headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    for part in value.split(',') {
        let mut fields = part.split(';');
        let tag = fields
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let language = if tag.starts_with("zh") {
            "zh-TW"
        } else if tag.starts_with("en") {
            "en"
        } else if tag.starts_with("es") {
            "es"
        } else {
            continue;
        };
        // 沒有 q（或 q 無法解析）視為 1
        let q = fields
            .find_map(|f| {
                let (name, v) = f.split_once('=')?;
                name.trim().eq_ignore_ascii_case("q").then(|| v.trim())
            })
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(1.0);
        if q > 0.0 && best.is_none_or(|(_, b)| q > b) {
            best = Some((language, q));
        }
    }
    best.map_or("zh-TW", |(language, _)| language)
}

/// 使用者的角色；每次請求由資料庫載入，不存在 cookie 裡（S-08.2）。
#[derive(Debug, Clone, Copy, Serialize, utoipa::ToSchema)]
pub struct Roles {
    pub admin: bool,
    pub teacher: bool,
    pub student: bool,
}

impl Roles {
    /// 教師或管理者。
    pub fn can_teach(&self) -> bool {
        self.admin || self.teacher
    }

    /// 能使用學生功能（選題、對話、語音、個人儀表板）：只有修課名單內的學生。
    /// 教師與管理者不參與討論，即使有教師或管理者身分也不行。
    pub fn is_participant(&self) -> bool {
        self.student
    }
}

/// 目前登入的使用者；沒有有效 session 時回 401，
/// 用臨時密碼登入、尚未更改密碼時回 403 `password_change_required`。
pub struct CurrentUser {
    pub user: auth_store::User,
    pub roles: Roles,
    /// 這個 session 是用臨時密碼登入、尚未更改密碼。
    pub must_change_password: bool,
}

/// 同 `CurrentUser`，但不要求已更改臨時密碼；只給 `/api/me` 與改密碼使用。
pub struct AnyCurrentUser(pub CurrentUser);

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let AnyCurrentUser(cu) = AnyCurrentUser::from_request_parts(parts, state).await?;
        if cu.must_change_password {
            return Err(ApiError::forbidden(
                "password_change_required",
                "請先更改密碼",
            ));
        }
        Ok(cu)
    }
}

impl FromRequestParts<AppState> for AnyCurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = token_from_headers(&parts.headers).ok_or_else(ApiError::unauthorized)?;
        // 學生：在任一教室的修課名單內即可（S-14）；不在任何名單內則是「尚未開通」
        let SessionUser {
            user,
            session,
            teacher,
            student,
        } = auth_store::find_active_session(&state.pool, &hash_token(token), Utc::now())
            .await?
            .ok_or_else(ApiError::unauthorized)?;
        // 停用的帳號即使還有 session 也不能使用（停用時雖已清除 session，這裡再擋一次）
        if user.disabled {
            return Err(ACCOUNT_DISABLED);
        }
        let roles = Roles {
            admin: user.is_admin,
            teacher,
            student,
        };
        Ok(AnyCurrentUser(CurrentUser {
            user,
            roles,
            must_change_password: session.must_change_password,
        }))
    }
}

/// 只有管理者可通過，否則 403。
pub struct RequireAdmin(pub CurrentUser);

impl FromRequestParts<AppState> for RequireAdmin {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let cu = CurrentUser::from_request_parts(parts, state).await?;
        if !cu.roles.admin {
            return Err(ApiError::forbidden("forbidden", "沒有權限"));
        }
        Ok(RequireAdmin(cu))
    }
}

/// 教師或管理者可通過，否則 403。
pub struct RequireTeacher(pub CurrentUser);

impl FromRequestParts<AppState> for RequireTeacher {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let cu = CurrentUser::from_request_parts(parts, state).await?;
        if !cu.roles.can_teach() {
            return Err(ApiError::forbidden("forbidden", "沒有權限"));
        }
        Ok(RequireTeacher(cu))
    }
}

/// 學生功能的守門：只有修課名單內的學生可通過。
/// 教師與管理者（不在名單內）回 403 `forbidden`，名單外帳號回 403 `not_enrolled`。
pub struct RequireStudent(pub CurrentUser);

impl FromRequestParts<AppState> for RequireStudent {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let cu = CurrentUser::from_request_parts(parts, state).await?;
        if !cu.roles.is_participant() {
            return Err(if cu.roles.can_teach() {
                ApiError::forbidden("forbidden", "教師與管理者不參與討論")
            } else {
                ApiError::forbidden("not_enrolled", "尚未開通")
            });
        }
        Ok(RequireStudent(cu))
    }
}

#[utoipa::path(
    get,
    path = "/api/auth/google/login",
    tag = "auth",
    summary = "導向 Google 授權頁",
    security(()),
    responses((status = 303, description = "重新導向到 Google"))
)]
pub(crate) async fn login(State(state): State<AppState>) -> Result<Redirect, ApiError> {
    let req = state.identity.start();
    auth_store::save_login_state(&state.pool, &req.state, &req.nonce, &req.pkce_verifier).await?;
    Ok(Redirect::to(&req.url))
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

fn login_error(state: &AppState, code: &str) -> Response {
    Redirect::to(&format!("{}/login?error={code}", state.config.app_base_url)).into_response()
}

#[utoipa::path(
    get,
    path = "/api/auth/google/callback",
    tag = "auth",
    summary = "Google 回呼（成功設定 sid cookie 並導回；失敗導回 /login?error=code）",
    params(CallbackParams),
    security(()),
    responses((status = 303, description = "重新導向"))
)]
pub(crate) async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(p): Query<CallbackParams>,
) -> Response {
    match finish_login(&state, p, preferred_language(&headers)).await {
        Ok(token) => (
            [(
                header::SET_COOKIE,
                session_cookie(&state, &token, SESSION_ABSOLUTE_TTL.num_seconds()),
            )],
            Redirect::to(&state.config.app_base_url),
        )
            .into_response(),
        Err(code) => login_error(&state, code),
    }
}

/// 成功時回傳新 session 的 token；失敗時回傳導回前端的錯誤代碼。
async fn finish_login(
    state: &AppState,
    p: CallbackParams,
    preferred_language: &str,
) -> Result<String, &'static str> {
    // state 一律取出並刪除（單次使用），即使 Google 回報了錯誤
    let stored = match p.state.as_deref() {
        Some(s) => auth_store::take_login_state(&state.pool, s, Utc::now())
            .await
            .map_err(|_| "login_failed")?,
        None => None,
    };
    if let Some(err) = p.error {
        return Err(if err == "access_denied" {
            "access_denied"
        } else {
            "login_failed"
        });
    }
    let (Some(stored), Some(code)) = (stored, p.code) else {
        return Err("invalid_state");
    };
    let identity = state
        .identity
        .finish(&code, &stored.nonce, &stored.pkce_verifier)
        .await
        .map_err(|_| "login_failed")?;
    if !identity.email_verified {
        return Err("email_not_verified");
    }
    let is_admin = state
        .config
        .admin_emails
        .contains(&identity.email.to_lowercase());
    let mut user = auth_store::upsert_user(
        &state.pool,
        &identity.sub,
        &identity.email,
        identity.name.as_deref(),
        is_admin,
    )
    .await
    .map_err(|_| "login_failed")?;
    if user.disabled {
        return Err("account_disabled");
    }
    user.language = auth_store::initialize_language(&state.pool, user.id, preferred_language)
        .await
        .map_err(|_| "login_failed")?;
    // 登入一律建立新 session（防止 session fixation）
    let token = new_token();
    auth_store::create_session(&state.pool, user.id, &hash_token(&token))
        .await
        .map_err(|_| "login_failed")?;
    audit(
        &state.pool,
        user.id,
        "login",
        user.email.as_deref().unwrap_or(""),
        None,
    )
    .await
    .map_err(|_| "login_failed")?;
    Ok(token)
}

#[utoipa::path(
    post,
    path = "/api/auth/logout",
    tag = "auth",
    summary = "登出",
    security(()),
    responses((status = 204, description = "已登出"))
)]
pub(crate) async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(token) = token_from_headers(&headers) {
        auth_store::delete_session(&state.pool, &hash_token(token)).await?;
    }
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, session_cookie(&state, "", 0))],
    )
        .into_response())
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct MeResponse {
    id: Uuid,
    email: Option<String>,
    display_name: Option<String>,
    language: String,
    is_admin: bool,
    must_change_password: bool,
    roles: Roles,
}

impl From<CurrentUser> for MeResponse {
    fn from(cu: CurrentUser) -> Self {
        let u = cu.user;
        Self {
            id: u.id,
            email: u.email,
            display_name: u.display_name,
            language: u.language,
            is_admin: u.is_admin,
            must_change_password: cu.must_change_password,
            roles: cu.roles,
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/me",
    tag = "auth",
    summary = "目前登入者與角色",
    responses((status = 200, description = "目前使用者", body = MeResponse))
)]
pub(crate) async fn me(AnyCurrentUser(cu): AnyCurrentUser) -> Json<MeResponse> {
    Json(cu.into())
}

/// 顯示名稱的長度上限（Unicode 字元數，去頭尾空白後計算；S-08.2 第 3 節）。
const DISPLAY_NAME_MAX_CHARS: usize = 30;

/// 去頭尾空白後的名稱是否可用：不超過上限，且不含控制字元（換行、tab、NUL 等）。
/// U+2028／U+2029（行／段落分隔，類別 Zl／Zp）不屬於 `is_control`，但同樣會破壞清單排版，一併拒絕。
fn valid_display_name(name: &str) -> bool {
    let breaks_layout = |c: char| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}');
    name.chars().count() <= DISPLAY_NAME_MAX_CHARS && !name.chars().any(breaks_layout)
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct MePatch {
    /// 沒帶＝保留；`null` 或空白字串＝恢復匯入姓名；上限 30 個字元，不可含控制字元。
    #[serde(default, deserialize_with = "nullable")]
    display_name: Option<Option<String>>,
    /// 介面與之後建立之對話的語言；僅接受 `zh-TW`、`en`、`es`。
    language: Option<String>,
}

#[utoipa::path(
    patch,
    path = "/api/me",
    tag = "auth",
    summary = "修改自己的語言與顯示名稱",
    request_body = MePatch,
    responses(
        (status = 200, description = "更新後的使用者", body = MeResponse),
        (status = 400, description = "顯示名稱無效（invalid_display_name）或語言不受支援（invalid_language）"),
        (status = 403, description = "非學生嘗試修改顯示名稱（所有登入者皆可修改語言）"),
    )
)]
pub(crate) async fn update_me(
    AnyCurrentUser(mut cu): AnyCurrentUser,
    State(state): State<AppState>,
    Json(p): Json<MePatch>,
) -> Result<Json<MeResponse>, ApiError> {
    if p.display_name.is_some() && !cu.roles.student {
        return Err(ApiError::forbidden("forbidden", "只有學生能修改顯示名稱"));
    }
    if p.language
        .as_deref()
        .is_some_and(|language| !matches!(language, "zh-TW" | "en" | "es"))
    {
        return Err(ApiError::bad_request("invalid_language", "不支援的語言"));
    }
    let name = p
        .display_name
        .map(|n| n.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()));
    if name.iter().flatten().any(|n| !valid_display_name(n)) {
        return Err(ApiError::bad_request(
            "invalid_display_name",
            "顯示名稱最多 30 個字元，且不可包含控制字元",
        ));
    }
    if name.is_none() && p.language.is_none() {
        return Ok(Json(cu.into()));
    }
    // 兩個欄位用同一條 UPDATE 寫入，一個 PATCH 不會只套用一半
    let (display_name, language): (Option<String>, String) = sqlx::query_as(
        "UPDATE users SET
             display_name = CASE WHEN $2 THEN COALESCE($3, imported_name) ELSE display_name END,
             language = COALESCE($4::text, language),
             language_initialized = language_initialized OR $4::text IS NOT NULL,
             updated_at = now()
         WHERE id = $1 RETURNING display_name, language",
    )
    .bind(cu.user.id)
    .bind(name.is_some())
    .bind(name.flatten())
    .bind(p.language)
    .fetch_one(&state.pool)
    .await?;
    cu.user.display_name = display_name;
    cu.user.language = language;
    Ok(Json(cu.into()))
}

// ---- 內建帳號密碼登入（S-08.2 第 1 節）----

/// 帳號不存在時也做一次雜湊比對，讓回應時間不洩漏帳號是否存在。
fn dummy_hash() -> &'static str {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| password::hash(&password::generate_temporary()).unwrap_or_default())
}

/// `hash` 為 `None` 時改比對 `dummy_hash`（第一次呼叫時的雜湊也在 blocking 執行緒上完成）。
async fn verify_blocking(password: String, hash: Option<String>) -> bool {
    tokio::task::spawn_blocking(move || {
        password::verify(&password, hash.as_deref().unwrap_or_else(|| dummy_hash()))
    })
    .await
    .unwrap_or(false)
}

pub async fn hash_blocking(password: String) -> Result<String, ApiError> {
    tokio::task::spawn_blocking(move || password::hash(&password))
        .await
        .ok()
        .and_then(Result::ok)
        .ok_or_else(ApiError::internal)
}

const INVALID_CREDENTIALS: ApiError = ApiError::new(
    StatusCode::UNAUTHORIZED,
    "invalid_credentials",
    "電子郵件或密碼不正確",
);

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct LoginBody {
    email: String,
    password: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct LoginResponse {
    must_change_password: bool,
}

#[utoipa::path(
    post,
    path = "/api/auth/login",
    tag = "auth",
    summary = "內建帳號登入",
    request_body = LoginBody,
    security(()),
    responses((status = 200, description = "成功並設定 sid cookie", body = LoginResponse), (status = 401, description = "invalid_credentials", body = crate::error::ErrorBody), (status = 403, description = "account_disabled", body = crate::error::ErrorBody), (status = 429, description = "too_many_attempts", body = crate::error::ErrorBody))
)]
pub(crate) async fn password_login(
    State(state): State<AppState>,
    connect_info: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(body): Json<LoginBody>,
) -> Result<Response, ApiError> {
    let email = body.email.trim().to_lowercase();
    let now = Utc::now();
    // 沒有連線資訊（例如測試）時 ip_key 為空字串：退回只以信箱計
    let ip_key = client_ip::client_key(
        connect_info.map(|Extension(ConnectInfo(addr))| addr.ip()),
        headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()),
        &state.config.trusted_proxies,
    );
    if auth_store::is_locked(&state.pool, &email, &ip_key, now).await? {
        return Err(login_rate_limit::TOO_MANY_ATTEMPTS);
    }
    let account = auth_store::find_password_account(&state.pool, &email).await?;
    let real_hash = account.as_ref().and_then(|a| a.password_hash.clone());
    let has_password = real_hash.is_some();
    let ok = verify_blocking(body.password, real_hash).await && has_password;
    let (Some(account), true) = (account, ok) else {
        auth_store::record_login_failure(&state.pool, &email, &ip_key, now).await?;
        return Err(INVALID_CREDENTIALS);
    };
    auth_store::clear_login_failures_for(&state.pool, &email, &ip_key).await?;
    // 密碼正確才揭露帳號已停用，不讓人用來探測帳號狀態
    if account.disabled {
        return Err(ACCOUNT_DISABLED);
    }
    auth_store::initialize_language(&state.pool, account.id, preferred_language(&headers)).await?;
    // 登入一律建立新 session（防止 session fixation）
    let token = new_token();
    let session =
        auth_store::create_password_session(&state.pool, account.id, &hash_token(&token)).await?;
    audit(&state.pool, account.id, "login", &email, None).await?;
    Ok((
        [(
            header::SET_COOKIE,
            session_cookie(&state, &token, SESSION_ABSOLUTE_TTL.num_seconds()),
        )],
        Json(LoginResponse {
            must_change_password: session.must_change_password,
        }),
    )
        .into_response())
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct ChangePasswordBody {
    current_password: String,
    new_password: String,
}

#[utoipa::path(
    post,
    path = "/api/auth/change-password",
    tag = "auth",
    summary = "更改自己的密碼",
    request_body = ChangePasswordBody,
    responses((status = 204, description = "已更改，其他 session 失效"), (status = 400, description = "invalid_current_password / invalid_new_password / password_unchanged", body = crate::error::ErrorBody))
)]
pub(crate) async fn change_password(
    AnyCurrentUser(cu): AnyCurrentUser,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ChangePasswordBody>,
) -> Result<StatusCode, ApiError> {
    if !password::is_acceptable(&body.new_password) {
        return Err(ApiError::bad_request(
            "invalid_new_password",
            "新密碼長度需為 8 到 128 個字元",
        ));
    }
    if body.new_password == body.current_password {
        return Err(ApiError::bad_request(
            "password_unchanged",
            "新密碼不能與目前密碼相同",
        ));
    }
    let wrong_current = ApiError::bad_request("invalid_current_password", "目前密碼不正確");
    let Some(current_hash) = auth_store::password_hash_of(&state.pool, cu.user.id).await? else {
        return Err(wrong_current);
    };
    if !verify_blocking(body.current_password, Some(current_hash)).await {
        return Err(wrong_current);
    }
    let new_hash = hash_blocking(body.new_password).await?;
    auth_store::set_password(&state.pool, cu.user.id, &new_hash, false).await?;
    // 保留目前登入，其他裝置的 session 全部失效
    let keep = token_from_headers(&headers).map(hash_token);
    auth_store::delete_sessions_except(&state.pool, cu.user.id, keep.as_ref().map(|h| &h[..]))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct ResetBody {
    email: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ResetResponse {
    email: String,
    /// 只在這次回應出現，之後無法再查詢。
    temporary_password: String,
}

/// 管理者重設密碼（產生臨時密碼）；信箱沒有帳號時一併建立。
#[utoipa::path(
    post,
    path = "/api/admin/users/reset-password",
    tag = "admin",
    summary = "重設密碼（僅管理者；沒有帳號就建立）",
    request_body = ResetBody,
    responses((status = 200, description = "臨時密碼只出現這一次", body = ResetResponse))
)]
pub(crate) async fn reset_password(
    RequireAdmin(cu): RequireAdmin,
    State(state): State<AppState>,
    Json(body): Json<ResetBody>,
) -> Result<Json<ResetResponse>, ApiError> {
    let email = crate::roster::normalize_email(&body.email)
        .ok_or(ApiError::bad_request("invalid_email", "電子郵件格式不正確"))?;
    let temporary_password = password::generate_temporary();
    let hash = hash_blocking(temporary_password.clone()).await?;
    let mut tx = state.pool.begin().await?;
    auth_store::reset_password(&mut tx, &email, &hash).await?;
    audit(&mut *tx, cu.user.id, "password_reset", &email, None).await?;
    tx.commit().await?;
    Ok(Json(ResetResponse {
        email,
        temporary_password,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(value: &str) -> &'static str {
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT_LANGUAGE, value.parse().unwrap());
        preferred_language(&headers)
    }

    #[test]
    fn accept_language_picks_the_highest_q_supported_language() {
        // 沒有 q 視為 1，同分取先出現的
        assert_eq!(pick("en, es"), "en");
        assert_eq!(pick("fr-FR, es-MX;q=0.9, en;q=0.8"), "es");
        assert_eq!(pick("en;q=0.1, es;q=0.9"), "es");
        assert_eq!(pick("en; q=0.2, zh-TW; q=0.7"), "zh-TW");
        assert_eq!(pick("zh-TW"), "zh-TW");
    }

    #[test]
    fn accept_language_ignores_languages_the_browser_refuses() {
        // q=0 代表「不接受」，即使排在前面也不能選
        assert_eq!(pick("es;q=0, en"), "en");
        assert_eq!(pick("en;q=0.000, es;q=0.1"), "es");
        // 沒有可接受的支援語言時回到繁體中文
        assert_eq!(pick("es;q=0"), "zh-TW");
        assert_eq!(pick("fr, *;q=0.5"), "zh-TW");
    }
}
