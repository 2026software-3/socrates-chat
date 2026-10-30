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
/// 同一信箱不論來源 IP，累計失敗幾次後在所有 IP 鎖定（分散式猜密碼的上限）。暫定值，待需求方確認（S-08.2、#295）。
pub const MAX_EMAIL_LOGIN_FAILURES: i32 = 30;
/// 鎖定時間。
pub const LOGIN_LOCK_DURATION: Duration = Duration::minutes(15);

#[derive(Debug, Clone, FromRow)]
pub struct User {
    pub id: Uuid,
    /// 尚未用 Google 登入過的內建帳號為 `None`。
    pub google_sub: Option<String>,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub language: String,
    pub is_admin: bool,
    /// 臨時密碼登入後必須先更改。
    pub must_change_password: bool,
    pub created_at: DateTime<Utc>,
    /// 管理者已停用此帳號（S-01.3）。
    pub disabled: bool,
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
    /// 信箱在教師名單內。
    pub teacher: bool,
    /// 信箱在任一教室的修課名單內（S-14）。
    pub student: bool,
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
/// 1. 已有相同 `sub` 的使用者：更新電子郵件（轉小寫）；顯示名稱只在尚未設定時才取自 Google，
///    不覆寫使用者自訂的名稱。
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
        "UPDATE users SET email = lower($2), display_name = COALESCE(display_name, $3),
                updated_at = now()
         WHERE google_sub = $1
         RETURNING id, google_sub, email, display_name, language, is_admin, must_change_password, created_at,
                   disabled_at IS NOT NULL AS disabled",
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
                 RETURNING id, google_sub, email, display_name, language, is_admin,
                           must_change_password, created_at, disabled_at IS NOT NULL AS disabled",
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
                         RETURNING id, google_sub, email, display_name, language, is_admin,
                                   must_change_password, created_at,
                                   disabled_at IS NOT NULL AS disabled",
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

/// 首次成功登入時，以瀏覽器偏好初始化帳號語言；之後登入不覆蓋使用者的設定。
pub async fn initialize_language(
    pool: &PgPool,
    user_id: Uuid,
    preferred: &str,
) -> Result<String, sqlx::Error> {
    // 先讀：第一次之後的每次登入只花這一次往返，而且不寫入。
    let (language, initialized): (String, bool) =
        sqlx::query_as("SELECT language, language_initialized FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await?;
    if initialized {
        return Ok(language);
    }
    let set: Option<String> = sqlx::query_scalar(
        "UPDATE users SET language = $2, language_initialized = true, updated_at = now()
         WHERE id = $1 AND NOT language_initialized
         RETURNING language",
    )
    .bind(user_id)
    .bind(preferred)
    .fetch_optional(pool)
    .await?;
    match set {
        Some(language) => Ok(language),
        // 兩個首次登入同時進來，另一個先寫入了：重新讀已提交的值。
        // （單一語句的 CTE 做不到：同一語句內的 SELECT 讀的是語句開始時的快照，會回傳舊值。）
        None => {
            sqlx::query_scalar("SELECT language FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_one(pool)
                .await
        }
    }
}

/// `find_active_session` 一次查回的 session、使用者與角色。
#[derive(FromRow)]
struct SessionUserRow {
    session_id: Uuid,
    session_created_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    session_must_change_password: bool,
    #[sqlx(flatten)]
    user: User,
    teacher: bool,
    student: bool,
}

/// 查詢有效的 session 並更新 `last_seen_at`，連同使用者與角色一起查回。
///
/// 閒置超過 7 天或建立超過 30 天視為失效（回傳 `None`）。每個需要登入的請求都會呼叫，
/// 所以合成一個語句，只需一次資料庫往返。
pub async fn find_active_session(
    pool: &PgPool,
    token_hash: &[u8],
    now: DateTime<Utc>,
) -> Result<Option<SessionUser>, sqlx::Error> {
    let row: Option<SessionUserRow> = sqlx::query_as(
        "WITH s AS (
             UPDATE sessions SET last_seen_at = $2
             WHERE token_hash = $1 AND last_seen_at > $3 AND created_at > $4
             RETURNING id, user_id, created_at, last_seen_at, must_change_password
         )
         SELECT s.id AS session_id, s.created_at AS session_created_at, s.last_seen_at,
                s.must_change_password AS session_must_change_password,
                u.id, u.google_sub, u.email, u.display_name, u.language, u.is_admin, u.must_change_password,
                u.created_at, u.disabled_at IS NOT NULL AS disabled,
                EXISTS (SELECT 1 FROM teachers t WHERE t.email = u.email) AS teacher,
                EXISTS (SELECT 1 FROM enrollments e WHERE e.user_id = u.id) AS student
         FROM s JOIN users u ON u.id = s.user_id",
    )
    .bind(token_hash)
    .bind(now)
    .bind(now - SESSION_IDLE_TTL)
    .bind(now - SESSION_ABSOLUTE_TTL)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| SessionUser {
        session: Session {
            id: r.session_id,
            user_id: r.user.id,
            created_at: r.session_created_at,
            last_seen_at: r.last_seen_at,
            must_change_password: r.session_must_change_password,
        },
        user: r.user,
        teacher: r.teacher,
        student: r.student,
    }))
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
    /// 沒有電子郵件的學生為 `None`（只能以學號登入）。
    pub email: Option<String>,
    pub password_hash: Option<String>,
    /// 管理者已停用此帳號（S-01.3）。
    pub disabled: bool,
}

/// 登入帳號輸入正規化：去前後空白；含 `@` 者轉小寫（電子郵件），否則轉大寫（學號）。
/// 限流與帳號比對都用正規化後的值，大小寫與空白的變化不能繞過計數（S-08.2）。
pub fn normalize_login_input(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.contains('@') {
        trimmed.to_lowercase()
    } else {
        trimmed.to_uppercase()
    }
}

/// 限流用的鍵：正規化後的登入帳號輸入的 SHA-256 十六進位，不存學號或電子郵件明文。
pub fn login_key(raw: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(normalize_login_input(raw).as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub async fn find_password_account(
    pool: &PgPool,
    account: &str,
) -> Result<Option<PasswordAccount>, sqlx::Error> {
    let normalized = normalize_login_input(account);
    // 含 `@` 以電子郵件比對；否則符合學號格式才以學號比對；兩者皆不符視為帳號不存在
    if normalized.contains('@') {
        sqlx::query_as(
            "SELECT id, email, password_hash, disabled_at IS NOT NULL AS disabled
             FROM users WHERE email = $1",
        )
        .bind(normalized)
        .fetch_optional(pool)
        .await
    } else if crate::roster::is_valid_student_id(&normalized) {
        sqlx::query_as(
            "SELECT id, email, password_hash, disabled_at IS NOT NULL AS disabled
             FROM users WHERE student_id = $1",
        )
        .bind(normalized)
        .fetch_optional(pool)
        .await
    } else {
        Ok(None)
    }
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

/// `login_failures` 裡代表「這個登入帳號輸入在所有 IP 的累計」的列（不會是任何真實的 `rate_limit_key`）。
const ACCOUNT_TOTAL_KEY: &str = "*";

/// 這個（登入帳號輸入, 來源 IP）或這個登入帳號輸入的跨 IP 總量目前是否被鎖定。`ip_key` 是
/// `client_ip::rate_limit_key` 的結果，沒有連線資訊時傳空字串。
pub async fn is_locked(
    pool: &PgPool,
    account: &str,
    ip_key: &str,
    now: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    let (locked,): (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM login_failures
                        WHERE login_key = $1 AND ip IN ($2, $3) AND locked_until > $4)",
    )
    .bind(login_key(account))
    .bind(ip_key)
    .bind(ACCOUNT_TOTAL_KEY)
    .bind(now)
    .fetch_one(pool)
    .await?;
    Ok(locked)
}

/// 記錄一次登入失敗：（登入帳號輸入, 來源 IP）累計達 `MAX_LOGIN_FAILURES` 次就鎖定 `LOGIN_LOCK_DURATION`；
/// 同時累計這個登入帳號輸入在所有 IP 的總數，達 `MAX_EMAIL_LOGIN_FAILURES` 次就在所有 IP 鎖定。
/// 鎖定已過期的紀錄會從 0 重新計。
pub async fn record_login_failure(
    pool: &PgPool,
    account: &str,
    ip_key: &str,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    // 順便清掉很久以前的紀錄，避免用大量不存在的信箱讓資料表無限成長
    sqlx::query("DELETE FROM login_failures WHERE last_failed_at < $1")
        .bind(now - Duration::days(1))
        .execute(pool)
        .await?;
    let key = login_key(account);
    bump_failures(pool, &key, ip_key, MAX_LOGIN_FAILURES, now).await?;
    bump_failures(pool, &key, ACCOUNT_TOTAL_KEY, MAX_EMAIL_LOGIN_FAILURES, now).await
}

async fn bump_failures(
    pool: &PgPool,
    login_key: &str,
    key: &str,
    max_failures: i32,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO login_failures (login_key, ip, failures, locked_until, last_failed_at)
         VALUES ($1, $2, 1, NULL, $3)
         ON CONFLICT (login_key, ip) DO UPDATE SET
            failures = CASE WHEN login_failures.locked_until IS NOT NULL
                                 AND login_failures.locked_until <= $3
                            THEN 1 ELSE login_failures.failures + 1 END,
            locked_until = CASE
                WHEN login_failures.locked_until IS NOT NULL AND login_failures.locked_until <= $3
                    THEN NULL
                WHEN login_failures.failures + 1 >= $4 THEN $5
                ELSE login_failures.locked_until END,
            last_failed_at = $3",
    )
    .bind(login_key)
    .bind(key)
    .bind(now)
    .bind(max_failures)
    .bind(now + LOGIN_LOCK_DURATION)
    .execute(pool)
    .await?;
    Ok(())
}

/// 成功登入：清掉這個（登入帳號輸入, 來源 IP）的計數與這個登入帳號輸入的跨 IP 總量；別的 IP
/// （可能是攻擊者）各自的計數不動。
pub async fn clear_login_failures_for(
    conn: impl sqlx::PgExecutor<'_>,
    account: &str,
    ip_key: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM login_failures WHERE login_key = $1 AND ip IN ($2, $3)")
        .bind(login_key(account))
        .bind(ip_key)
        .bind(ACCOUNT_TOTAL_KEY)
        .execute(conn)
        .await?;
    Ok(())
}

/// 清掉這個帳號所有登入帳號輸入（學號與電子郵件）在所有來源 IP 的計數
/// （管理者重設密碼、使用者改密碼、刪除帳號時）。
pub async fn clear_login_failures_of_user(
    conn: &mut sqlx::PgConnection,
    user_id: Uuid,
) -> Result<(), sqlx::Error> {
    let row: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT email, student_id FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&mut *conn)
            .await?;
    let keys: Vec<String> = row
        .into_iter()
        .flat_map(|(email, student_id)| [email, student_id])
        .flatten()
        .map(|account| login_key(&account))
        .collect();
    sqlx::query("DELETE FROM login_failures WHERE login_key = ANY($1)")
        .bind(keys)
        .execute(&mut *conn)
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
    let mut conn = pool.acquire().await?;
    clear_login_failures_of_user(&mut conn, user_id).await?;
    Ok(())
}

/// 刪除使用者的 session；`keep` 指定要保留的那一個（改密碼時保留目前登入）。
pub async fn delete_sessions_except(
    conn: impl sqlx::PgExecutor<'_>,
    user_id: Uuid,
    keep: Option<&[u8]>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM sessions WHERE user_id = $1 AND ($2::bytea IS NULL OR token_hash <> $2)",
    )
    .bind(user_id)
    .bind(keep)
    .execute(conn)
    .await?;
    Ok(())
}

/// 管理者重設密碼：沒有帳號就建立（不是管理者、沒有 Google 綁定）；有就更新。
/// 設定臨時密碼、清除所有 session 與鎖定。回傳使用者 id。
/// 呼叫端須傳入交易中的連線，並與稽核紀錄一起提交。
pub async fn reset_password(
    conn: &mut sqlx::PgConnection,
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
    .fetch_one(&mut *conn)
    .await?;
    delete_sessions_except(&mut *conn, id, None).await?;
    clear_login_failures_of_user(&mut *conn, id).await?;
    Ok(id)
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

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use sqlx::postgres::PgPoolOptions;

    use super::*;

    /// 只有一條連線的連線池，每次從池裡取連線就記一次。直接用 `&PgPool` 執行的每條語句
    /// 取一次連線，所以次數就是登入熱路徑的資料庫往返次數。
    async fn counting_pool(pool: &PgPool) -> (PgPool, Arc<AtomicUsize>) {
        let acquires = Arc::new(AtomicUsize::new(0));
        let counter = acquires.clone();
        let counting = PgPoolOptions::new()
            .max_connections(1)
            .before_acquire(move |_, _| {
                counter.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(true) })
            })
            .connect_with((*pool.connect_options()).clone())
            .await
            .unwrap();
        acquires.store(0, Ordering::SeqCst);
        (counting, acquires)
    }

    async fn seed_user(pool: &PgPool) -> Uuid {
        sqlx::query_scalar("INSERT INTO users (email) VALUES ('lang@example.com') RETURNING id")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[sqlx::test]
    async fn initialize_language_costs_one_round_trip_after_the_first_login(pool: PgPool) {
        let user = seed_user(&pool).await;
        let (counting, acquires) = counting_pool(&pool).await;

        let first = initialize_language(&counting, user, "es").await.unwrap();
        assert_eq!(first, "es");
        assert_eq!(
            acquires.swap(0, Ordering::SeqCst),
            2,
            "first login: read + set"
        );

        // 之後的登入不覆蓋已初始化的語言，而且只花一次往返
        let later = initialize_language(&counting, user, "en").await.unwrap();
        assert_eq!(later, "es");
        assert_eq!(acquires.swap(0, Ordering::SeqCst), 1, "later logins");
    }

    #[sqlx::test]
    async fn racing_first_logins_both_return_the_committed_language(pool: PgPool) {
        let user = seed_user(&pool).await;
        // 贏家已更新但還沒 commit；輸家的語句會卡在列鎖上，等贏家 commit 後必須回傳贏家寫入的值
        let mut winner = pool.begin().await.unwrap();
        sqlx::query("UPDATE users SET language = 'es', language_initialized = true WHERE id = $1")
            .bind(user)
            .execute(&mut *winner)
            .await
            .unwrap();
        let loser = tokio::spawn({
            let pool = pool.clone();
            async move { initialize_language(&pool, user, "en").await }
        });
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        winner.commit().await.unwrap();
        assert_eq!(loser.await.unwrap().unwrap(), "es");
    }

    #[sqlx::test]
    async fn initialize_language_for_a_missing_user_is_row_not_found(pool: PgPool) {
        let err = initialize_language(&pool, Uuid::new_v4(), "es")
            .await
            .unwrap_err();
        assert!(matches!(err, sqlx::Error::RowNotFound));
    }
}
