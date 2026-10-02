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
};

const COOKIE_NAME: &str = "sid";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/google/login", get(login))
        .route("/api/auth/google/callback", get(callback))
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

/// 目前登入的使用者；沒有有效 session 時回 401。
pub struct CurrentUser {
    pub user: auth_store::User,
    pub roles: Roles,
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = token_from_headers(&parts.headers).ok_or_else(ApiError::unauthorized)?;
        let SessionUser { user, .. } =
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
        Ok(CurrentUser { user, roles })
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
    roles: Roles,
}

async fn me(cu: CurrentUser) -> Json<MeResponse> {
    let u = cu.user;
    Json(MeResponse {
        id: u.id,
        email: u.email,
        display_name: u.display_name,
        is_admin: u.is_admin,
        roles: cu.roles,
    })
}
