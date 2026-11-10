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

/// 活動（table alias `a`）的 `topic_ids`（依順序的 JSON 陣列）與相容舊欄位 `topic_id`（第一個題目）。
/// PostgreSQL 版用 `LATERAL` 加 `array_agg`；SQLite 用相關子查詢與 `json_group_array`。
macro_rules! topic_columns {
    () => {
        "(SELECT json_group_array(topic_id) FROM
             (SELECT topic_id FROM activity_topics WHERE activity_id = a.id ORDER BY position)) AS topic_ids,
         (SELECT topic_id FROM activity_topics WHERE activity_id = a.id ORDER BY position LIMIT 1) AS topic_id"
    };
}

macro_rules! activity_select {
    ($tail:literal) => {
        concat!(
            "SELECT a.id, a.title, a.description, ",
            topic_columns!(),
            ", a.classroom_id, a.status, a.is_archived, a.opens_at, a.closes_at, a.created_at
             FROM activities a ",
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
         WHERE ($1 OR cl.created_by = $2) AND ($3 IS NULL OR a.classroom_id = $3)
           AND a.is_archived = $4
           AND ($5 IS NULL
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
    // SQLite 不能在 CTE 裡 INSERT：活動與題目在同一個 D1 batch 內寫入（原子），再讀回。
    let pool = sqlx::Executor::into_pool(executor);
    let id = Uuid::new_v4();
    let mut writes = vec![
        sqlx::query(
            "INSERT INTO activities (id, title, description, classroom_id, created_by, opens_at, closes_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(id)
        .bind(title)
        .bind(description)
        .bind(classroom_id)
        .bind(created_by)
        .bind(opens_at)
        .bind(closes_at),
    ];
    for (position, topic_id) in topic_ids.iter().enumerate() {
        writes.push(
            sqlx::query(
                "INSERT INTO activity_topics (activity_id, topic_id, position) VALUES ($1, $2, $3)",
            )
            .bind(id)
            .bind(topic_id)
            .bind(position as i64),
        );
    }
    pool.batch(writes).await?;
    find_by_id(&pool, id).await
}

/// 改變狀態；活動不存在、不在操作者可管理的教室、教室或活動已封存時回 `None`（呼叫端再分辨原因）。
pub async fn set_status_writable(
    pool: &PgPool,
    id: Uuid,
    actor_id: Uuid,
    is_admin: bool,
    status: &str,
) -> Result<Option<Activity>, sqlx::Error> {
    // SQLite 不能在 CTE 裡 UPDATE：先更新，有更新到再讀回。
    let updated: Option<Uuid> = sqlx::query_scalar(
        "UPDATE activities SET status = $2, updated_at = now()
         WHERE id = $1 AND NOT is_archived
           AND EXISTS (SELECT 1 FROM classrooms cl
                       WHERE cl.id = activities.classroom_id AND ($3 OR cl.created_by = $4)
                         AND NOT cl.is_archived)
         RETURNING id",
    )
    .bind(id)
    .bind(status)
    .bind(is_admin)
    .bind(actor_id)
    .fetch_optional(pool)
    .await?;
    match updated {
        Some(id) => find_by_id(pool, id).await.map(Some),
        None => Ok(None),
    }
}

/// 學生可選的活動：已發布、未封存、在開放期間內（以資料庫時間計算），且所在教室的名單內有這位學生。
pub async fn list_published_for_student(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<Activity>, sqlx::Error> {
    sqlx::query_as(activity_select!(
        "WHERE a.status = 'published' AND NOT a.is_archived
           AND (a.opens_at IS NULL OR now() >= a.opens_at)
           AND (a.closes_at IS NULL OR now() < a.closes_at)
           AND NOT EXISTS (SELECT 1 FROM classrooms cl WHERE cl.id = a.classroom_id AND cl.is_archived)
           AND EXISTS (SELECT 1 FROM enrollments e
                       WHERE e.classroom_id = a.classroom_id AND e.user_id = $1)
         ORDER BY a.created_at DESC, a.id"
    ))
    .bind(user_id)
    .fetch_all(pool)
    .await
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

/// 學生看到的活動：活動本身加上每題進度與整體完成度。
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct StudentActivity {
    #[serde(flatten)]
    pub activity: Activity,
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
    let ids: Vec<Uuid> = activities.iter().map(|a| a.id).collect();
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
         WHERE at.activity_id IN (SELECT value FROM json_each($2))
         ORDER BY at.activity_id, at.position",
    )
    .bind(user_id)
    .bind(&ids)
    .fetch_all_rows(pool)
    .await?
    .iter()
    .map(|row| Ok((row.try_get("activity_id")?, row.decode::<TopicProgress>()?)))
    .collect::<Result<_, sqlx::Error>>()?;
    let mut by_activity: std::collections::HashMap<Uuid, Vec<TopicProgress>> = Default::default();
    for (activity_id, progress) in rows {
        by_activity.entry(activity_id).or_default().push(progress);
    }
    Ok(activities
        .into_iter()
        .map(|activity| {
            let topics = by_activity.remove(&activity.id).unwrap_or_default();
            let completed = !topics.is_empty() && topics.iter().all(|t| t.summary_ready);
            StudentActivity {
                activity,
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
        "SELECT a.id, t.id AS topic_id, a.classroom_id,
                CASE WHEN $3 IS NULL THEN a.title ELSE t.title END AS title,
                CASE WHEN $3 IS NULL THEN a.description ELSE t.description END AS description
         FROM activities a
         JOIN topics t ON t.id = (SELECT topic_id FROM activity_topics
                                  WHERE activity_id = a.id AND ($3 IS NULL OR topic_id = $3)
                                  ORDER BY position LIMIT 1)
         JOIN classrooms cl ON cl.id = a.classroom_id
         WHERE a.id = $1 AND a.status = 'published' AND NOT a.is_archived AND NOT cl.is_archived
           AND (a.opens_at IS NULL OR now() >= a.opens_at)
           AND (a.closes_at IS NULL OR now() < a.closes_at)
           AND EXISTS (SELECT 1 FROM enrollments e
                       WHERE e.classroom_id = a.classroom_id AND e.user_id = $2)",
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
    let pool = sqlx::Executor::into_pool(conn);
    sqlx::query("UPDATE activities SET is_archived = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(archived)
        .execute(&pool)
        .await?;
    find_by_id(&pool, id).await
}
