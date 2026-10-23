//! F-28.2 修課名單資料存取：名單屬於教室，同一電子郵件可在多個教室（S-14、S-01.4）。
//!
//! 電子郵件一律小寫；呼叫端負責驗證教室的可見性（見 `classroom_store::get_visible`）。

use serde::Serialize;
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub struct StudentRow {
    /// 使用者 UUID；尚未建立帳號時為 null。
    pub id: Option<Uuid>,
    pub email: String,
    pub display_name: Option<String>,
    /// 學生是否已有帳號（登入過，或管理者匯入時建立了內建帳號）
    pub has_account: bool,
    pub disabled: bool,
    /// 已完成（總結已產生）的對話數；教師只看得到數量，看不到內容
    pub completed_conversations: i64,
}

pub async fn list_emails(pool: &PgPool, classroom_id: Uuid) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT email FROM enrollments WHERE classroom_id = $1 ORDER BY email")
        .bind(classroom_id)
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
        "INSERT INTO enrollments (classroom_id, email) VALUES ($1, lower($2))
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
        sqlx::query("DELETE FROM enrollments WHERE classroom_id = $1 AND email = lower($2)")
            .bind(classroom_id)
            .bind(email)
            .execute(conn)
            .await?;
    Ok(result.rows_affected() > 0)
}

/// 帳號是否在任一教室的名單內；不在任何名單內的帳號登入後看到「尚未開通」。
pub async fn is_enrolled_anywhere(pool: &PgPool, email: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM enrollments WHERE email = lower($1))")
        .bind(email)
        .fetch_one(pool)
        .await
}

/// 教室名單上的學生與帳號狀態。
pub async fn list_students(
    pool: &PgPool,
    classroom_id: Uuid,
) -> Result<Vec<StudentRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT u.id, e.email, u.display_name,
                (u.id IS NOT NULL) AS has_account,
                COALESCE(u.disabled_at IS NOT NULL, false) AS disabled,
                COALESCE((SELECT count(*) FROM summaries s
                          JOIN conversations c ON c.id = s.conversation_id
                          WHERE c.user_id = u.id AND s.status = 'ready'), 0)
                    AS completed_conversations
         FROM enrollments e LEFT JOIN users u ON u.email = e.email
         WHERE e.classroom_id = $1
         ORDER BY e.email",
    )
    .bind(classroom_id)
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
        "SELECT count(DISTINCT e.email)
         FROM enrollments e JOIN classrooms c ON c.id = e.classroom_id
         WHERE $1 OR c.created_by = $2",
    )
    .bind(is_admin)
    .bind(actor_id)
    .fetch_one(pool)
    .await
}
