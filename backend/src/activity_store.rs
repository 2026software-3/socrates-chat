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
    /// 活動題目清單（S-04.7、F-29.1）
    pub topic_ids: Vec<Uuid>,
    /// 相容舊欄位（第一個題目；沒有題目時為 null）
    pub topic_id: Option<Uuid>,
    /// 活動所屬的教室，建立後不可更改（S-14）
    pub classroom_id: Uuid,
    pub status: String,
    /// 封存：學生看不到、不能開新對話，既有對話照舊（S-14）
    pub is_archived: bool,
    pub opens_at: Option<DateTime<Utc>>,
    pub closes_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// `topic_ids`（依順序）與相容舊欄位 `topic_id`（第一個題目）兩個輸出欄位；需搭配 `topic_join!`。
macro_rules! topic_columns {
    () => {
        "COALESCE(tp.ids, ARRAY[]::uuid[]) AS topic_ids, tp.ids[1] AS topic_id"
    };
}

/// 取得活動（table alias 為 `$alias`）的有序題目陣列 `tp.ids`。
macro_rules! topic_join {
    ($alias:literal) => {
        concat!(
            "LEFT JOIN LATERAL (SELECT array_agg(topic_id ORDER BY position) AS ids
                                FROM activity_topics WHERE activity_id = ",
            $alias,
            ".id) tp ON true "
        )
    };
}

macro_rules! activity_select {
    ($tail:literal) => {
        concat!(
            "SELECT a.id, a.title, a.description, ",
            topic_columns!(),
            ", a.classroom_id, a.status, a.is_archived, a.opens_at, a.closes_at, a.created_at
             FROM activities a ",
            topic_join!("a"),
            $tail
        )
    };
}

/// 操作者看得到的活動，依 `created_at DESC, id ASC`，從 `after`（不含）之後取 `limit` 筆；
/// `classroom` 有值時只看那間教室（呼叫端須先確認教室可見）。
pub async fn list(
    pool: &PgPool,
    actor_id: Uuid,
    is_admin: bool,
    classroom: Option<Uuid>,
    archived: bool,
    after: Option<(DateTime<Utc>, Uuid)>,
    limit: i64,
) -> Result<Vec<Activity>, sqlx::Error> {
    let (after_time, after_id) = after.unzip();
    sqlx::query_as(activity_select!(
        "JOIN classrooms cl ON cl.id = a.classroom_id
         WHERE ($1 OR cl.created_by = $2) AND ($3::uuid IS NULL OR a.classroom_id = $3)
           AND a.is_archived = $4
           AND ($5::timestamptz IS NULL
                OR a.created_at < $5 OR (a.created_at = $5 AND a.id > $6))
         ORDER BY a.created_at DESC, a.id ASC LIMIT $7"
    ))
    .bind(is_admin)
    .bind(actor_id)
    .bind(classroom)
    .bind(archived)
    .bind(after_time)
    .bind(after_id)
    .bind(limit)
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

pub async fn find_by_id<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    id: Uuid,
) -> Result<Activity, sqlx::Error> {
    sqlx::query_as(activity_select!("WHERE a.id = $1"))
        .bind(id)
        .fetch_one(executor)
        .await
}

pub async fn create_with_schedule<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    classroom_id: Uuid,
    created_by: Uuid,
    title: &str,
    description: &str,
    topic_ids: &[Uuid],
    schedule: (Option<DateTime<Utc>>, Option<DateTime<Utc>>),
) -> Result<Activity, sqlx::Error> {
    let (opens_at, closes_at) = schedule;
    sqlx::query_as(
        "WITH a AS (
            INSERT INTO activities (title, description, classroom_id, created_by, opens_at, closes_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, title, description, classroom_id, status, is_archived, opens_at, closes_at, created_at
         ),
         ins AS (
            INSERT INTO activity_topics (activity_id, topic_id, position)
            SELECT a.id, t.topic_id, (t.ord - 1)::integer
            FROM a
            CROSS JOIN LATERAL unnest($7::uuid[]) WITH ORDINALITY AS t(topic_id, ord)
         )
         SELECT a.id, a.title, a.description,
                $7::uuid[] AS topic_ids,
                ($7::uuid[])[1] AS topic_id,
                a.classroom_id, a.status, a.is_archived, a.opens_at, a.closes_at, a.created_at
         FROM a",
    )
    .bind(title)
    .bind(description)
    .bind(classroom_id)
    .bind(created_by)
    .bind(opens_at)
    .bind(closes_at)
    .bind(topic_ids)
    .fetch_one(executor)
    .await
}

/// 改變狀態；活動不存在、不在操作者可管理的教室、教室或活動已封存時回 `None`（呼叫端再分辨原因）。
pub async fn set_status_writable(
    pool: &PgPool,
    id: Uuid,
    actor_id: Uuid,
    is_admin: bool,
    status: &str,
) -> Result<Option<Activity>, sqlx::Error> {
    sqlx::query_as(concat!(
        "WITH upd AS (
            UPDATE activities a SET status = $2, updated_at = now()
            FROM classrooms cl
            WHERE a.id = $1 AND cl.id = a.classroom_id AND ($3 OR cl.created_by = $4)
              AND NOT cl.is_archived AND NOT a.is_archived
            RETURNING a.id, a.title, a.description, a.classroom_id, a.status, a.is_archived, a.opens_at, a.closes_at, a.created_at
         )
         SELECT u.id, u.title, u.description, ",
        topic_columns!(),
        ", u.classroom_id, u.status, u.is_archived, u.opens_at, u.closes_at, u.created_at
         FROM upd u ",
        topic_join!("u")
    ))
    .bind(id)
    .bind(status)
    .bind(is_admin)
    .bind(actor_id)
    .fetch_optional(pool)
    .await
}

/// 學生可選的活動：已發布、未封存、在開放期間內（以資料庫時間計算），且所在教室的名單內有這位學生。
pub async fn list_published_for_student(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<PublishedActivity>, sqlx::Error> {
    // 同一個查詢帶出教室名稱：活動都在學生名單內的教室，所以只會有學生自己所在教室的名稱（S-14）
    sqlx::query_as(concat!(
        "SELECT a.id, a.title, a.description, ",
        topic_columns!(),
        ", a.classroom_id, a.status, a.is_archived, a.opens_at, a.closes_at, a.created_at,
           cl.name AS classroom_name
         FROM activities a JOIN classrooms cl ON cl.id = a.classroom_id ",
        topic_join!("a"),
        "WHERE a.status = 'published' AND NOT a.is_archived AND NOT cl.is_archived
           AND (a.opens_at IS NULL OR statement_timestamp() >= a.opens_at)
           AND (a.closes_at IS NULL OR statement_timestamp() < a.closes_at)
           AND EXISTS (SELECT 1 FROM enrollments e
                       WHERE e.classroom_id = a.classroom_id AND e.user_id = $1)
         ORDER BY a.created_at DESC, a.id"
    ))
    .bind(user_id)
    .fetch_all(pool)
    .await
}

/// 學生可選的已發布活動，多帶所屬教室名稱（S-14）。
/// 學生看得到的教室資訊只有名稱，不含教師、教室說明或其他學生。
#[derive(Debug, FromRow)]
pub struct PublishedActivity {
    #[sqlx(flatten)]
    pub activity: Activity,
    pub classroom_name: String,
}

/// 學生在活動中某一題的進度（F-29.6）。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub struct TopicProgress {
    pub topic_id: Uuid,
    pub title: String,
    /// 這位學生在此題的對話；還沒開始為 null
    pub conversation_id: Option<Uuid>,
    /// `active` 或 `ended`；還沒開始為 null
    pub conversation_status: Option<String>,
    /// 此題的總結已產生（S-04.7：完成一題的條件）
    pub summary_ready: bool,
}

/// 學生看到的活動：活動本身加上所屬教室名稱（供前端依教室分組，S-14）、每題進度與整體完成度。
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct StudentActivity {
    #[serde(flatten)]
    pub activity: Activity,
    /// 所屬教室名稱；學生看得到的教室資訊只有名稱
    pub classroom_name: String,
    /// 依教師排定的順序，每題一筆
    pub topics: Vec<TopicProgress>,
    /// 每一題都有已產生總結的對話才算完成（S-04.7）
    pub completed: bool,
}

/// 學生可選的活動，附上該學生在每題的對話狀態。
pub async fn list_published_with_progress(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<StudentActivity>, sqlx::Error> {
    let activities = list_published_for_student(pool, user_id).await?;
    let ids: Vec<Uuid> = activities.iter().map(|a| a.activity.id).collect();
    let rows: Vec<(Uuid, TopicProgress)> = sqlx::query(
        "SELECT at.activity_id, at.topic_id, t.title,
                conv.id AS conversation_id, conv.status AS conversation_status,
                COALESCE(sm.status = 'ready', false) AS summary_ready
         FROM activity_topics at
         JOIN topics t ON t.id = at.topic_id
         LEFT JOIN conversations conv
                ON conv.activity_id = at.activity_id AND conv.topic_id = at.topic_id
               AND conv.user_id = $1
         LEFT JOIN summaries sm ON sm.conversation_id = conv.id
         WHERE at.activity_id = ANY($2)
         ORDER BY at.activity_id, at.position",
    )
    .bind(user_id)
    .bind(&ids)
    .try_map(|row: sqlx::postgres::PgRow| {
        use sqlx::Row;
        Ok((row.try_get("activity_id")?, TopicProgress::from_row(&row)?))
    })
    .fetch_all(pool)
    .await?;
    let mut by_activity: std::collections::HashMap<Uuid, Vec<TopicProgress>> = Default::default();
    for (activity_id, progress) in rows {
        by_activity.entry(activity_id).or_default().push(progress);
    }
    Ok(activities
        .into_iter()
        .map(|published| {
            let topics = by_activity
                .remove(&published.activity.id)
                .unwrap_or_default();
            let completed = !topics.is_empty() && topics.iter().all(|t| t.summary_ready);
            StudentActivity {
                activity: published.activity,
                classroom_name: published.classroom_name,
                topics,
                completed,
            }
        })
        .collect())
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
/// 教師修改活動題目清單時會鎖同一列再檢查有沒有對話，所以新對話與題目變更不會同時成立（S-04.7）。
/// 活動不存在、未發布、沒有題目或學生不在名單內都回 `None`，呼叫端不應區分原因，以免外露資源是否存在。
pub async fn find_startable(
    conn: &mut PgConnection,
    id: Uuid,
    user_id: Uuid,
) -> Result<Option<Startable>, sqlx::Error> {
    find_startable_topic(conn, id, user_id, None).await
}

/// 指定題目必須屬於活動；未指定時暫時沿用舊客戶端的第一題行為。
pub async fn find_startable_topic(
    conn: &mut PgConnection,
    id: Uuid,
    user_id: Uuid,
    topic_id: Option<Uuid>,
) -> Result<Option<Startable>, sqlx::Error> {
    sqlx::query_as(
        "SELECT a.id, ft.topic_id, a.classroom_id,
                t.title,
                CASE WHEN $3::uuid IS NULL THEN a.description ELSE t.description END
         FROM activities a
         JOIN LATERAL (SELECT topic_id FROM activity_topics
                       WHERE activity_id = a.id AND ($3::uuid IS NULL OR topic_id = $3)
                       ORDER BY position LIMIT 1) ft ON true
         JOIN topics t ON t.id = ft.topic_id
         JOIN classrooms cl ON cl.id = a.classroom_id
         WHERE a.id = $1 AND a.status = 'published' AND NOT a.is_archived AND NOT cl.is_archived
           AND (a.opens_at IS NULL OR statement_timestamp() >= a.opens_at)
           AND (a.closes_at IS NULL OR statement_timestamp() < a.closes_at)
           AND EXISTS (SELECT 1 FROM enrollments e
                       WHERE e.classroom_id = a.classroom_id AND e.user_id = $2)
         FOR SHARE OF a, cl",
    )
    .bind(id)
    .bind(user_id)
    .bind(topic_id)
    .fetch_optional(conn)
    .await
}

/// 封存或復原活動；呼叫端已在交易內鎖住該列並確認操作者可管理。
pub async fn set_archived<'e>(
    conn: impl sqlx::PgExecutor<'e>,
    id: Uuid,
    archived: bool,
) -> Result<Activity, sqlx::Error> {
    sqlx::query_as(concat!(
        "WITH upd AS (
            UPDATE activities SET is_archived = $2, updated_at = now() WHERE id = $1
            RETURNING id, title, description, classroom_id, status, is_archived, opens_at, closes_at, created_at
         )
         SELECT u.id, u.title, u.description, ",
        topic_columns!(),
        ", u.classroom_id, u.status, u.is_archived, u.opens_at, u.closes_at, u.created_at
         FROM upd u ",
        topic_join!("u")
    ))
    .bind(id)
    .bind(archived)
    .fetch_one(conn)
    .await
}
