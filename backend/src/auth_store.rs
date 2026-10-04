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
/// 內建登入連續失敗幾次後鎖定。
pub const MAX_LOGIN_FAILURES: i32 = 5;
/// 鎖定時間。
pub const LOGIN_LOCK_DURATION: Duration = Duration::minutes(15);

#[derive(Debug, Clone, FromRow)]
pub struct User {
    pub id: Uuid,
    /// 尚未用 Google 登入過的內建帳號為 `None`。
    pub google_sub: Option<String>,
    pub email: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
    /// 臨時密碼登入後必須先更改。
    pub must_change_password: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    /// 用臨時密碼登入、尚未更改密碼的 session。
    pub must_change_password: bool,
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

/// 依 Google 帳號建立、更新或綁定使用者。
///
/// 1. 已有相同 `sub` 的使用者：更新電子郵件（轉小寫）與顯示名稱。
/// 2. 否則，電子郵件已有尚未綁定 Google 的內建帳號：綁定 `sub`（呼叫端須確認 `email_verified`）。
///    信箱的擁有者本人登入了，所以：仍是臨時密碼（從未更改）的帳號會清掉密碼，建立者知道的臨時密碼
///    不能繼續用；`admin_on_create`（信箱在 ADMIN_EMAILS）時授予管理者。使用者自己改過的密碼不受影響。
/// 3. 否則建立新使用者。
///
/// 已綁定的使用者**不會**被改動 `is_admin`；`admin_on_create` 只在首次建立或首次綁定時生效（S-08.2）。
pub async fn upsert_user(
    pool: &PgPool,
    google_sub: &str,
    email: &str,
    display_name: Option<&str>,
    admin_on_create: bool,
) -> Result<User, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let existing: Option<User> = sqlx::query_as(
        "UPDATE users SET email = lower($2), display_name = $3, updated_at = now()
         WHERE google_sub = $1
         RETURNING id, google_sub, email, display_name, is_admin, must_change_password, created_at",
    )
    .bind(google_sub)
    .bind(email)
    .bind(display_name)
    .fetch_optional(&mut *tx)
    .await?;
    let user = match existing {
        Some(u) => u,
        None => {
            let bound: Option<User> = sqlx::query_as(
                "UPDATE users SET google_sub = $1,
                        display_name = COALESCE(display_name, $3),
                        is_admin = is_admin OR $4,
                        password_hash = CASE WHEN must_change_password THEN NULL
                                             ELSE password_hash END,
                        must_change_password = false,
                        updated_at = now()
                 WHERE email = lower($2) AND google_sub IS NULL
                 RETURNING id, google_sub, email, display_name, is_admin,
                           must_change_password, created_at",
            )
            .bind(google_sub)
            .bind(email)
            .bind(display_name)
            .bind(admin_on_create)
            .fetch_optional(&mut *tx)
            .await?;
            match bound {
                Some(u) => u,
                None => {
                    sqlx::query_as(
                        "INSERT INTO users (google_sub, email, display_name, is_admin)
                         VALUES ($1, lower($2), $3, $4)
                         RETURNING id, google_sub, email, display_name, is_admin,
                                   must_change_password, created_at",
                    )
                    .bind(google_sub)
                    .bind(email)
                    .bind(display_name)
                    .bind(admin_on_create)
                    .fetch_one(&mut *tx)
                    .await?
                }
            }
        }
    };
    tx.commit().await?;
    Ok(user)
}

/// 建立新 session；`token_hash` 為 256 位元 token 的 SHA-256 雜湊。
pub async fn create_session(
    pool: &PgPool,
    user_id: Uuid,
    token_hash: &[u8],
) -> Result<Session, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO sessions (user_id, token_hash) VALUES ($1, $2)
         RETURNING id, user_id, created_at, last_seen_at, must_change_password",
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
         RETURNING id, user_id, created_at, last_seen_at, must_change_password",
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
        "SELECT id, google_sub, email, display_name, is_admin, must_change_password, created_at
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
    // 順便清掉放棄的登入流程，避免資料表無限成長
    sqlx::query("DELETE FROM oauth_login_states WHERE created_at < now() - interval '10 minutes'")
        .execute(pool)
        .await?;
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

// ---- 內建帳號密碼登入（S-08.2 第 1 節）----

/// 內建登入用的帳號與雜湊；沒有密碼（只用 Google）的使用者 `password_hash` 為 `None`。
#[derive(Debug, Clone, FromRow)]
pub struct PasswordAccount {
    pub id: Uuid,
    pub password_hash: Option<String>,
}

pub async fn find_password_account(
    pool: &PgPool,
    email: &str,
) -> Result<Option<PasswordAccount>, sqlx::Error> {
    sqlx::query_as("SELECT id, password_hash FROM users WHERE email = lower($1)")
        .bind(email)
        .fetch_optional(pool)
        .await
}

/// 讀取目前密碼雜湊（改密碼時驗證舊密碼用）。
pub async fn password_hash_of(pool: &PgPool, user_id: Uuid) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT password_hash FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(h,)| h))
}

/// 這個電子郵件目前是否被鎖定。
pub async fn is_locked(
    pool: &PgPool,
    email: &str,
    now: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    let (locked,): (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM login_failures
                        WHERE email = lower($1) AND locked_until > $2)",
    )
    .bind(email)
    .bind(now)
    .fetch_one(pool)
    .await?;
    Ok(locked)
}

/// 記錄一次登入失敗；累計達 `MAX_LOGIN_FAILURES` 次就鎖定 `LOGIN_LOCK_DURATION`。
/// 鎖定已過期的紀錄會從 0 重新計。
pub async fn record_login_failure(
    pool: &PgPool,
    email: &str,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    // 順便清掉很久以前的紀錄，避免用大量不存在的信箱讓資料表無限成長
    sqlx::query("DELETE FROM login_failures WHERE last_failed_at < $1")
        .bind(now - Duration::days(1))
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO login_failures (email, failures, locked_until, last_failed_at)
         VALUES (lower($1), 1, NULL, $2)
         ON CONFLICT (email) DO UPDATE SET
            failures = CASE WHEN login_failures.locked_until IS NOT NULL
                                 AND login_failures.locked_until <= $2
                            THEN 1 ELSE login_failures.failures + 1 END,
            locked_until = CASE
                WHEN login_failures.locked_until IS NOT NULL AND login_failures.locked_until <= $2
                    THEN NULL
                WHEN login_failures.failures + 1 >= $3 THEN $4
                ELSE login_failures.locked_until END,
            last_failed_at = $2",
    )
    .bind(email)
    .bind(now)
    .bind(MAX_LOGIN_FAILURES)
    .bind(now + LOGIN_LOCK_DURATION)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn clear_login_failures(pool: &PgPool, email: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM login_failures WHERE email = lower($1)")
        .bind(email)
        .execute(pool)
        .await?;
    Ok(())
}

/// 設定密碼：同時清除鎖定。`must_change` 為 true 表示這是臨時密碼。
pub async fn set_password(
    pool: &PgPool,
    user_id: Uuid,
    password_hash: &str,
    must_change: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE users SET password_hash = $2, must_change_password = $3, updated_at = now()
         WHERE id = $1",
    )
    .bind(user_id)
    .bind(password_hash)
    .bind(must_change)
    .execute(pool)
    .await?;
    if !must_change {
        // 已改成自己的密碼：這個使用者目前的 session 不再受限
        sqlx::query("UPDATE sessions SET must_change_password = false WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await?;
    }
    sqlx::query("DELETE FROM login_failures WHERE email = (SELECT email FROM users WHERE id = $1)")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// 刪除使用者的 session；`keep` 指定要保留的那一個（改密碼時保留目前登入）。
pub async fn delete_sessions_except(
    pool: &PgPool,
    user_id: Uuid,
    keep: Option<&[u8]>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM sessions WHERE user_id = $1 AND ($2::bytea IS NULL OR token_hash <> $2)",
    )
    .bind(user_id)
    .bind(keep)
    .execute(pool)
    .await?;
    Ok(())
}

/// 管理者重設密碼：沒有帳號就建立（不是管理者、沒有 Google 綁定）；有就更新。
/// 設定臨時密碼、清除所有 session 與鎖定。回傳使用者 id。
pub async fn reset_password(
    pool: &PgPool,
    email: &str,
    password_hash: &str,
) -> Result<Uuid, sqlx::Error> {
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO users (email, password_hash, must_change_password)
         VALUES (lower($1), $2, true)
         ON CONFLICT (email) DO UPDATE SET
            password_hash = EXCLUDED.password_hash, must_change_password = true, updated_at = now()
         RETURNING id",
    )
    .bind(email)
    .bind(password_hash)
    .fetch_one(pool)
    .await?;
    delete_sessions_except(pool, id, None).await?;
    clear_login_failures(pool, email).await?;
    Ok(id)
}

/// 匯入名單時為新加入的學生建立內建帳號；信箱已有帳號（例如用 Google 登入過）時不動，回傳 `false`。
pub async fn create_account_if_absent(
    pool: &PgPool,
    email: &str,
    password_hash: &str,
) -> Result<bool, sqlx::Error> {
    let r = sqlx::query(
        "INSERT INTO users (email, password_hash, must_change_password)
         VALUES (lower($1), $2, true) ON CONFLICT (email) DO NOTHING",
    )
    .bind(email)
    .bind(password_hash)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

/// 部署時依 `ADMIN_EMAILS` 建立首位管理者的內建帳號（初始密碼來自環境變數，首次登入須更改）。
/// 已有帳號的信箱不會被改動（重啟不會重設密碼）；回傳新建立的數量。
pub async fn bootstrap_admins(
    pool: &PgPool,
    emails: &[String],
    password_hash: &str,
) -> Result<u64, sqlx::Error> {
    let mut created = 0;
    for email in emails {
        let r = sqlx::query(
            "INSERT INTO users (email, password_hash, must_change_password, is_admin)
             VALUES (lower($1), $2, true, true) ON CONFLICT (email) DO NOTHING",
        )
        .bind(email)
        .bind(password_hash)
        .execute(pool)
        .await?;
        created += r.rows_affected();
    }
    Ok(created)
}

/// 密碼登入建立的 session：使用者還在用臨時密碼時，session 帶有「須更改密碼」旗標。
pub async fn create_password_session(
    pool: &PgPool,
    user_id: Uuid,
    token_hash: &[u8],
) -> Result<Session, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO sessions (user_id, token_hash, must_change_password)
         SELECT id, $2, must_change_password FROM users WHERE id = $1
         RETURNING id, user_id, created_at, last_seen_at, must_change_password",
    )
    .bind(user_id)
    .bind(token_hash)
    .fetch_one(pool)
    .await
}
