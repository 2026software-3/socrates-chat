//! 登入與 session 的資料存取層（F-01.1-DB，規格見 S-08.2）。
//!
//! 時間一律由呼叫端傳入 `now`，方便測試；token 雜湊由呼叫端（後端）計算。

use chrono::{DateTime, Duration, Utc};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

/// Session 閒置上限。
pub const SESSION_IDLE_TTL: Duration = Duration::days(7);
/// Session 絕對上限。
pub const SESSION_ABSOLUTE_TTL: Duration = Duration::days(30);
/// OAuth state 有效時間。
pub const LOGIN_STATE_TTL: Duration = Duration::minutes(10);

#[derive(Debug, Clone, FromRow)]
pub struct User {
    pub id: Uuid,
    pub google_sub: String,
    pub email: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct SessionUser {
    pub session: Session,
    pub user: User,
}

#[derive(Debug, Clone, FromRow)]
pub struct LoginState {
    pub state: String,
    pub nonce: String,
    pub pkce_verifier: String,
    pub created_at: DateTime<Utc>,
}

/// 依 Google 帳號 ID 建立或更新使用者。
///
/// 已存在時更新電子郵件（轉小寫）與顯示名稱，**不會**改動 `is_admin`；
/// `admin_on_create` 只在首次建立時生效（ADMIN_EMAILS，S-08.2）。
pub async fn upsert_user(
    pool: &PgPool,
    google_sub: &str,
    email: &str,
    display_name: Option<&str>,
    admin_on_create: bool,
) -> Result<User, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO users (google_sub, email, display_name, is_admin)
         VALUES ($1, lower($2), $3, $4)
         ON CONFLICT (google_sub) DO UPDATE
            SET email = EXCLUDED.email,
                display_name = EXCLUDED.display_name,
                updated_at = now()
         RETURNING id, google_sub, email, display_name, is_admin, created_at",
    )
    .bind(google_sub)
    .bind(email)
    .bind(display_name)
    .bind(admin_on_create)
    .fetch_one(pool)
    .await
}

/// 建立新 session；`token_hash` 為 256 位元 token 的 SHA-256 雜湊。
pub async fn create_session(
    pool: &PgPool,
    user_id: Uuid,
    token_hash: &[u8],
) -> Result<Session, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO sessions (user_id, token_hash) VALUES ($1, $2)
         RETURNING id, user_id, created_at, last_seen_at",
    )
    .bind(user_id)
    .bind(token_hash)
    .fetch_one(pool)
    .await
}

/// 查詢有效的 session 並更新 `last_seen_at`。
///
/// 閒置超過 7 天或建立超過 30 天視為失效（回傳 `None`）。
pub async fn find_active_session(
    pool: &PgPool,
    token_hash: &[u8],
    now: DateTime<Utc>,
) -> Result<Option<SessionUser>, sqlx::Error> {
    let session: Option<Session> = sqlx::query_as(
        "UPDATE sessions SET last_seen_at = $2
         WHERE token_hash = $1 AND last_seen_at > $3 AND created_at > $4
         RETURNING id, user_id, created_at, last_seen_at",
    )
    .bind(token_hash)
    .bind(now)
    .bind(now - SESSION_IDLE_TTL)
    .bind(now - SESSION_ABSOLUTE_TTL)
    .fetch_optional(pool)
    .await?;
    let Some(session) = session else {
        return Ok(None);
    };
    let user = sqlx::query_as(
        "SELECT id, google_sub, email, display_name, is_admin, created_at
         FROM users WHERE id = $1",
    )
    .bind(session.user_id)
    .fetch_one(pool)
    .await?;
    Ok(Some(SessionUser { session, user }))
}

/// 刪除 session（登出）；回傳是否有刪到。
pub async fn delete_session(pool: &PgPool, token_hash: &[u8]) -> Result<bool, sqlx::Error> {
    let r = sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
        .bind(token_hash)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

/// 暫存 OAuth 授權流程的 state、nonce 與 PKCE verifier。
pub async fn save_login_state(
    pool: &PgPool,
    state: &str,
    nonce: &str,
    pkce_verifier: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO oauth_login_states (state, nonce, pkce_verifier) VALUES ($1, $2, $3)")
        .bind(state)
        .bind(nonce)
        .bind(pkce_verifier)
        .execute(pool)
        .await?;
    Ok(())
}

/// 取出並刪除 state 紀錄（單次使用）；不存在或超過 10 分鐘回傳 `None`（過期的紀錄仍會被刪除）。
pub async fn take_login_state(
    pool: &PgPool,
    state: &str,
    now: DateTime<Utc>,
) -> Result<Option<LoginState>, sqlx::Error> {
    let row: Option<LoginState> = sqlx::query_as(
        "DELETE FROM oauth_login_states WHERE state = $1
         RETURNING state, nonce, pkce_verifier, created_at",
    )
    .bind(state)
    .fetch_optional(pool)
    .await?;
    Ok(row.filter(|s| s.created_at > now - LOGIN_STATE_TTL))
}
