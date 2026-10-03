//! 登入、登出與 session 驗證（F-01.1，規格見 S-08.2）。

use axum::{
    Json, Router,
    extract::{FromRequestParts, Query, State},
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
    AppState, auth_store,
    auth_store::{SESSION_ABSOLUTE_TTL, SessionUser},
    error::ApiError,
    password,
};

const COOKIE_NAME: &str = "sid";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/google/login", get(login))
        .route("/api/auth/google/callback", get(callback))
        .route("/api/auth/login", post(password_login))
        .route("/api/auth/change-password", post(change_password))
        .route("/api/admin/users/reset-password", post(reset_password))
        .route("/api/auth/logout", post(logout))
        .route("/api/me", get(me))
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

/// 使用者的角色；每次請求由資料庫載入，不存在 cookie 裡（S-08.2）。
#[derive(Debug, Clone, Copy, Serialize)]
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

    /// 能使用學生功能：修課名單內的學生，或教師、管理者（不需要在名單內）。
    pub fn is_member(&self) -> bool {
        self.admin || self.teacher || self.student
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
        let SessionUser { user, session } =
            auth_store::find_active_session(&state.pool, &hash_token(token), Utc::now())
                .await?
                .ok_or_else(ApiError::unauthorized)?;
        let (teacher, student): (bool, bool) = sqlx::query_as(
            "SELECT EXISTS (SELECT 1 FROM teachers WHERE email = $1),
                    EXISTS (SELECT 1 FROM enrollments WHERE email = $1)",
        )
        .bind(&user.email)
        .fetch_one(&state.pool)
        .await?;
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

/// 學生功能的守門：名單外、非教師、非管理者回 403 `not_enrolled`。
pub struct RequireStudent(pub CurrentUser);

impl FromRequestParts<AppState> for RequireStudent {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let cu = CurrentUser::from_request_parts(parts, state).await?;
        if !cu.roles.is_member() {
            return Err(ApiError::forbidden("not_enrolled", "尚未開通"));
        }
        Ok(RequireStudent(cu))
    }
}

async fn login(State(state): State<AppState>) -> Result<Redirect, ApiError> {
    let req = state.identity.start();
    auth_store::save_login_state(&state.pool, &req.state, &req.nonce, &req.pkce_verifier).await?;
    Ok(Redirect::to(&req.url))
}

#[derive(Deserialize)]
struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

fn login_error(state: &AppState, code: &str) -> Response {
    Redirect::to(&format!("{}/login?error={code}", state.config.app_base_url)).into_response()
}

async fn callback(State(state): State<AppState>, Query(p): Query<CallbackParams>) -> Response {
    match finish_login(&state, p).await {
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
async fn finish_login(state: &AppState, p: CallbackParams) -> Result<String, &'static str> {
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
    let user = auth_store::upsert_user(
        &state.pool,
        &identity.sub,
        &identity.email,
        identity.name.as_deref(),
        is_admin,
    )
    .await
    .map_err(|_| "login_failed")?;
    // 登入一律建立新 session（防止 session fixation）
    let token = new_token();
    auth_store::create_session(&state.pool, user.id, &hash_token(&token))
        .await
        .map_err(|_| "login_failed")?;
    Ok(token)
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    if let Some(token) = token_from_headers(&headers) {
        auth_store::delete_session(&state.pool, &hash_token(token)).await?;
    }
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, session_cookie(&state, "", 0))],
    )
        .into_response())
}

#[derive(Serialize)]
struct MeResponse {
    id: Uuid,
    email: String,
    display_name: Option<String>,
    is_admin: bool,
    must_change_password: bool,
    roles: Roles,
}

async fn me(AnyCurrentUser(cu): AnyCurrentUser) -> Json<MeResponse> {
    let u = cu.user;
    Json(MeResponse {
        id: u.id,
        email: u.email,
        display_name: u.display_name,
        is_admin: u.is_admin,
        must_change_password: cu.must_change_password,
        roles: cu.roles,
    })
}

// ---- 內建帳號密碼登入（S-08.2 第 1 節）----

/// 帳號不存在時也做一次雜湊比對，讓回應時間不洩漏帳號是否存在。
fn dummy_hash() -> &'static str {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| password::hash(&password::generate_temporary()).unwrap_or_default())
}

async fn verify_blocking(password: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || password::verify(&password, &hash))
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

#[derive(Deserialize)]
struct LoginBody {
    email: String,
    password: String,
}

#[derive(Serialize)]
struct LoginResponse {
    must_change_password: bool,
}

async fn password_login(
    State(state): State<AppState>,
    Json(body): Json<LoginBody>,
) -> Result<Response, ApiError> {
    let email = body.email.trim().to_lowercase();
    let now = Utc::now();
    if auth_store::is_locked(&state.pool, &email, now).await? {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_attempts",
            "嘗試次數過多，請稍後再試",
        ));
    }
    let account = auth_store::find_password_account(&state.pool, &email).await?;
    let real_hash = account.as_ref().and_then(|a| a.password_hash.clone());
    let has_password = real_hash.is_some();
    let hash = real_hash.unwrap_or_else(|| dummy_hash().to_string());
    let ok = verify_blocking(body.password, hash).await && has_password;
    let (Some(account), true) = (account, ok) else {
        auth_store::record_login_failure(&state.pool, &email, now).await?;
        return Err(INVALID_CREDENTIALS);
    };
    auth_store::clear_login_failures(&state.pool, &email).await?;
    // 登入一律建立新 session（防止 session fixation）
    let token = new_token();
    let session =
        auth_store::create_password_session(&state.pool, account.id, &hash_token(&token)).await?;
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

#[derive(Deserialize)]
struct ChangePasswordBody {
    current_password: String,
    new_password: String,
}

async fn change_password(
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
    if !verify_blocking(body.current_password, current_hash).await {
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

#[derive(Deserialize)]
struct ResetBody {
    email: String,
}

#[derive(Serialize)]
struct ResetResponse {
    email: String,
    /// 只在這次回應出現，之後無法再查詢。
    temporary_password: String,
}

/// 管理者重設密碼（產生臨時密碼）；信箱沒有帳號時一併建立。
async fn reset_password(
    _: RequireAdmin,
    State(state): State<AppState>,
    Json(body): Json<ResetBody>,
) -> Result<Json<ResetResponse>, ApiError> {
    let email = crate::roster::normalize_email(&body.email)
        .ok_or(ApiError::bad_request("invalid_email", "電子郵件格式不正確"))?;
    let temporary_password = password::generate_temporary();
    let hash = hash_blocking(temporary_password.clone()).await?;
    auth_store::reset_password(&state.pool, &email, &hash).await?;
    Ok(Json(ResetResponse {
        email,
        temporary_password,
    }))
}
