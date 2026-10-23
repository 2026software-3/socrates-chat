//! F-28.3 活動資料存取：活動屬於教室（S-14）；題目是全域的，活動只引用它。
//!
//! 教師端的查詢一律經過教室（`JOIN classrooms`）：教師只看得到自己建立的教室，管理者看全部。
//! 學生端只看得到自己所在教室名單內、已發布的活動。

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub struct Activity {
    pub id: Uuid,
    pub title: String,
    pub description: String,
    /// 活動必須選題庫題目（S-04.7）
    pub topic_id: Uuid,
    /// 活動所屬的教室，建立後不可更改（S-14）
    pub classroom_id: Uuid,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

macro_rules! activity_select {
    ($tail:literal) => {
        concat!(
            "SELECT a.id, a.title, a.description, a.topic_id, a.classroom_id, a.status, a.created_at
             FROM activities a ",
            $tail
        )
    };
}

/// 操作者看得到的活動；`classroom` 有值時只看那間教室（呼叫端須先確認教室可見）。
pub async fn list(
    pool: &PgPool,
    actor_id: Uuid,
    is_admin: bool,
    classroom: Option<Uuid>,
) -> Result<Vec<Activity>, sqlx::Error> {
    sqlx::query_as(activity_select!(
        "JOIN classrooms cl ON cl.id = a.classroom_id
         WHERE ($1 OR cl.created_by = $2) AND ($3::uuid IS NULL OR a.classroom_id = $3)
         ORDER BY a.created_at DESC, a.id"
    ))
    .bind(is_admin)
    .bind(actor_id)
    .bind(classroom)
    .fetch_all(pool)
    .await
}

pub async fn get_visible(
    pool: &PgPool,
    id: Uuid,
    actor_id: Uuid,
    is_admin: bool,
) -> Result<Option<Activity>, sqlx::Error> {
    sqlx::query_as(activity_select!(
        "JOIN classrooms cl ON cl.id = a.classroom_id
         WHERE a.id = $1 AND ($2 OR cl.created_by = $3)"
    ))
    .bind(id)
    .bind(is_admin)
    .bind(actor_id)
    .fetch_optional(pool)
    .await
}

pub async fn create(
    pool: &PgPool,
    classroom_id: Uuid,
    created_by: Uuid,
    title: &str,
    description: &str,
    topic_id: Uuid,
) -> Result<Activity, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO activities (title, description, topic_id, classroom_id, created_by)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, title, description, topic_id, classroom_id, status, created_at",
    )
    .bind(title)
    .bind(description)
    .bind(topic_id)
    .bind(classroom_id)
    .bind(created_by)
    .fetch_one(pool)
    .await
}

/// 改變狀態；活動不存在或不在操作者可管理的教室時回 `None`。
pub async fn set_status_visible(
    pool: &PgPool,
    id: Uuid,
    actor_id: Uuid,
    is_admin: bool,
    status: &str,
) -> Result<Option<Activity>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE activities a SET status = $2, updated_at = now()
         FROM classrooms cl
         WHERE a.id = $1 AND cl.id = a.classroom_id AND ($3 OR cl.created_by = $4)
         RETURNING a.id, a.title, a.description, a.topic_id, a.classroom_id, a.status, a.created_at",
    )
    .bind(id)
    .bind(status)
    .bind(is_admin)
    .bind(actor_id)
    .fetch_optional(pool)
    .await
}

/// 學生可選的活動：已發布，且所在教室的名單內有這位學生。
pub async fn list_published_for_student(
    pool: &PgPool,
    email: &str,
) -> Result<Vec<Activity>, sqlx::Error> {
    sqlx::query_as(activity_select!(
        "WHERE a.status = 'published'
           AND EXISTS (SELECT 1 FROM enrollments e
                       WHERE e.classroom_id = a.classroom_id AND e.email = lower($1))
         ORDER BY a.created_at DESC, a.id"
    ))
    .bind(email)
    .fetch_all(pool)
    .await
}

/// 學生可以開始對話的活動。
#[derive(Debug, FromRow)]
pub struct Startable {
    pub id: Uuid,
    pub topic_id: Uuid,
    pub classroom_id: Uuid,
    pub title: String,
    pub description: String,
}

/// 找出學生能開始對話的活動（已發布，且學生在活動所屬教室的名單內），並以 `FOR SHARE` 鎖住活動列：
/// 教師更換活動題目時會鎖同一列再檢查有沒有對話，所以新對話與更換題目不會同時成立（S-04.7）。
/// 活動不存在、未發布或學生不在名單內都回 `None`，呼叫端不應區分原因，以免外露資源是否存在。
pub async fn find_startable(
    conn: &mut PgConnection,
    id: Uuid,
    email: &str,
) -> Result<Option<Startable>, sqlx::Error> {
    sqlx::query_as(
        "SELECT a.id, a.topic_id, a.classroom_id, a.title, a.description FROM activities a
         WHERE a.id = $1 AND a.status = 'published'
           AND EXISTS (SELECT 1 FROM enrollments e
                       WHERE e.classroom_id = a.classroom_id AND e.email = lower($2))
         FOR SHARE OF a",
    )
    .bind(id)
    .bind(email)
    .fetch_optional(conn)
    .await
}
