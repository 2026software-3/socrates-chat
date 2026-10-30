//! F-28.2 修課名單資料存取：名單屬於教室，同一電子郵件可在多個教室（S-14、S-01.4）。
//!
//! 電子郵件一律小寫；呼叫端負責驗證教室的可見性（見 `classroom_store::get_visible`）。

use serde::Serialize;
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

/// 將學生帳號加入教室；相同帳號在同一教室只保留一列。
pub async fn add_user(
    conn: &mut PgConnection,
    classroom_id: Uuid,
    user_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO enrollments (classroom_id, user_id, student_id)
         SELECT $1, id, student_id FROM users WHERE id = $2 ON CONFLICT DO NOTHING",
    )
    .bind(classroom_id)
    .bind(user_id)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// 以學號找既有帳號並鎖住，確保同時匯入時不會建立重複帳號。
pub async fn find_import_account(
    conn: &mut PgConnection,
    student_id: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM users WHERE student_id = $1 FOR UPDATE")
        .bind(student_id)
        .fetch_optional(conn)
        .await
}

pub async fn create_student(
    conn: &mut PgConnection,
    student_id: &str,
    name: &str,
    password_hash: &str,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO users (student_id, imported_name, display_name, password_hash, must_change_password)
         VALUES ($1, $2, $2, $3, true) RETURNING id",
    )
    .bind(student_id)
    .bind(name)
    .bind(password_hash)
    .fetch_one(conn)
    .await
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub struct StudentRow {
    pub id: Uuid,
    pub email: Option<String>,
    pub student_id: String,
    pub display_name: Option<String>,
    /// 學生是否已有帳號（登入過，或管理者匯入時建立了內建帳號）
    pub has_account: bool,
    pub disabled: bool,
    /// 這間教室內已完成（總結已產生）的活動對話數，不含個人練習；教師只看得到數量，看不到內容
    pub completed_conversations: i64,
}

/// 依帳號 UUID 排序的名單；UUID 不會因補上電子郵件而改變。
#[derive(Serialize, FromRow, utoipa::ToSchema)]
pub struct RosterEntry {
    pub id: Uuid,
    pub email: Option<String>,
}

pub async fn list_roster(
    pool: &PgPool,
    classroom_id: Uuid,
    after: Option<Uuid>,
    limit: i64,
) -> Result<Vec<RosterEntry>, sqlx::Error> {
    sqlx::query_as(
        "SELECT u.id, u.email FROM enrollments e JOIN users u ON u.id = e.user_id
         WHERE e.classroom_id = $1 AND ($2::uuid IS NULL OR u.id > $2)
         ORDER BY u.id LIMIT $3",
    )
    .bind(classroom_id)
    .bind(after)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// 加入教室名單；已在名單內回 `false`。
pub async fn add(
    conn: &mut PgConnection,
    classroom_id: Uuid,
    email: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO enrollments (classroom_id, user_id, student_id) SELECT $1, id, student_id FROM users WHERE email = lower($2)
         ON CONFLICT DO NOTHING",
    )
    .bind(classroom_id)
    .bind(email)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// 移出教室名單（只影響這間教室）；原本就不在名單內回 `false`。
pub async fn remove(
    conn: &mut PgConnection,
    classroom_id: Uuid,
    email: &str,
) -> Result<bool, sqlx::Error> {
    let result =
        sqlx::query("DELETE FROM enrollments WHERE classroom_id = $1 AND user_id = (SELECT id FROM users WHERE email = lower($2))")
            .bind(classroom_id)
            .bind(email)
            .execute(conn)
            .await?;
    Ok(result.rows_affected() > 0)
}

/// 以帳號 UUID 或電子郵件移出名單，回傳被移出的帳號 UUID。
pub async fn remove_user(
    conn: &mut PgConnection,
    classroom_id: Uuid,
    identifier: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "DELETE FROM enrollments e USING users u
         WHERE e.classroom_id = $1 AND e.user_id = u.id
           AND (u.id::text = $2 OR u.email = lower($2)) RETURNING u.id",
    )
    .bind(classroom_id)
    .bind(identifier)
    .fetch_optional(conn)
    .await
}

/// 帳號是否在任一教室的名單內；不在任何名單內的帳號登入後看到「尚未開通」。
pub async fn is_enrolled_anywhere(pool: &PgPool, email: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM enrollments e JOIN users u ON u.id = e.user_id WHERE u.email = lower($1))")
        .bind(email)
        .fetch_one(pool)
        .await
}

/// 這個帳號是否至少在 `owner_id` 建立的一間教室內，且所有教室都是 `owner_id` 建立的。
/// 刪除帳號會連同所有教室的資料一併刪除，所以教師只能刪除完全屬於自己教室的學生。
pub async fn user_belongs_only_to_owner(
    conn: &mut PgConnection,
    user_id: Uuid,
    owner_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT bool_or(c.created_by = $2) AND bool_and(c.created_by = $2)
         FROM enrollments e JOIN classrooms c ON c.id = e.classroom_id
         WHERE e.user_id = $1",
    )
    .bind(user_id)
    .bind(owner_id)
    .fetch_one(conn)
    .await
    .map(|owned: Option<bool>| owned.unwrap_or(false))
}

/// 教室名單上的學生與帳號狀態，依帳號 UUID 排序，從 `after`（不含）之後取 `limit` 筆；
/// `search` 為 `ILIKE` 樣式（見 `pagination::like_pattern`），只比對電子郵件與顯示名稱。
pub async fn list_students(
    pool: &PgPool,
    classroom_id: Uuid,
    after: Option<Uuid>,
    limit: i64,
    search: Option<&str>,
) -> Result<Vec<StudentRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT u.id, u.email, u.student_id, u.display_name,
                true AS has_account,
                (u.disabled_at IS NOT NULL) AS disabled,
                (SELECT count(*) FROM summaries s
                          JOIN conversations c ON c.id = s.conversation_id
                          WHERE c.user_id = u.id AND c.classroom_id = e.classroom_id
                            AND s.status = 'ready')
                    AS completed_conversations
         FROM enrollments e JOIN users u ON u.id = e.user_id
         WHERE e.classroom_id = $1 AND ($2::uuid IS NULL OR u.id > $2)
           AND ($4::text IS NULL
                OR u.email ILIKE $4 ESCAPE '\\'
                OR u.display_name ILIKE $4 ESCAPE '\\')
         ORDER BY u.id LIMIT $3",
    )
    .bind(classroom_id)
    .bind(after)
    .bind(limit)
    .bind(search)
    .fetch_all(pool)
    .await
}

/// 操作者看得到的教室內的學生人數；同一帳號在多個教室只算一人。
/// 教師只算自己建立的教室，管理者算全部。
pub async fn count_students(
    pool: &PgPool,
    actor_id: Uuid,
    is_admin: bool,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(DISTINCT e.user_id)
         FROM enrollments e JOIN classrooms c ON c.id = e.classroom_id
         WHERE $1 OR c.created_by = $2",
    )
    .bind(is_admin)
    .bind(actor_id)
    .fetch_one(pool)
    .await
}

/// 這位學生在這間教室內已完成（總結已產生）的活動對話數；個人練習與其他教室不計入。
pub async fn count_completed(
    pool: &PgPool,
    classroom_id: Uuid,
    email: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM summaries s
         JOIN conversations c ON c.id = s.conversation_id
         JOIN users u ON u.id = c.user_id
         WHERE c.classroom_id = $1 AND u.email = lower($2) AND s.status = 'ready'",
    )
    .bind(classroom_id)
    .bind(email)
    .fetch_one(pool)
    .await
}

/// 教室名單上符合搜尋條件的學生人數（條件同 `list_students`，不受游標影響）。
pub async fn count_students_matching(
    pool: &PgPool,
    classroom_id: Uuid,
    search: Option<&str>,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*)
         FROM enrollments e JOIN users u ON u.id = e.user_id
         WHERE e.classroom_id = $1
           AND ($2::text IS NULL
                OR u.email ILIKE $2 ESCAPE '\\'
                OR u.display_name ILIKE $2 ESCAPE '\\')",
    )
    .bind(classroom_id)
    .bind(search)
    .fetch_one(pool)
    .await
}

/// 單一教室名單上的學生人數。
pub async fn count_in_classroom(pool: &PgPool, classroom_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM enrollments WHERE classroom_id = $1")
        .bind(classroom_id)
        .fetch_one(pool)
        .await
}
