use chrono::{DateTime, Utc};
use sqlx::{Acquire, FromRow, PgConnection, PgPool, Postgres, Transaction, types::Json};
use uuid::Uuid;

use crate::book_store::{self, BookWork};

use super::{Recommendation, RecommendationStatus, selection};

#[derive(FromRow)]
pub(super) struct Source {
    pub id: Uuid,
    pub user_id: Uuid,
    pub classroom_id: Option<Uuid>,
    pub activity_id: Option<Uuid>,
    pub topic_id: Option<Uuid>,
    pub title: String,
    pub guiding_questions: Vec<String>,
    pub language: String,
    pub stance: Option<String>,
    pub reasons: Option<String>,
    pub claim: Option<String>,
    pub eligible: bool,
}

macro_rules! eligible_sql {
    () => { "
    u.disabled_at IS NULL AND NOT u.is_admin
    AND NOT EXISTS (SELECT 1 FROM teachers t WHERE t.email = u.email)
    AND (c.classroom_id IS NULL OR (
        EXISTS (SELECT 1 FROM classrooms cl WHERE cl.id = c.classroom_id AND NOT cl.is_archived)
        AND EXISTS (SELECT 1 FROM enrollments e WHERE e.classroom_id = c.classroom_id AND e.user_id = c.user_id)
    ))
    AND (c.activity_id IS NULL OR EXISTS (
        SELECT 1 FROM activities a WHERE a.id = c.activity_id AND NOT a.is_archived
    ))" };
}

pub(super) async fn eligible(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query_scalar(concat!(
        "SELECT ",
        eligible_sql!(),
        " FROM conversations c JOIN users u ON u.id = c.user_id WHERE c.id = $1"
    ))
    .bind(id)
    .fetch_optional(conn)
    .await?
    .unwrap_or(false))
}

pub(super) async fn source(pool: &PgPool, id: Uuid) -> Result<Option<Source>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT c.id, c.user_id, c.classroom_id, c.activity_id, c.topic_id, c.title,
                COALESCE(t.guiding_questions, '{}') AS guiding_questions, c.language,
                s.stance, s.reasons, s.claim, (",
        eligible_sql!(),
        ") AS eligible
         FROM conversations c JOIN users u ON u.id = c.user_id
         JOIN summaries s ON s.conversation_id = c.id AND s.status = 'ready'
         LEFT JOIN topics t ON t.id = c.topic_id
         WHERE c.id = $1 AND c.status = 'ended'"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}

#[derive(FromRow)]
pub(super) struct StateRow {
    pub status: RecommendationStatus,
    pub retry_at: Option<DateTime<Utc>>,
    pub error_code: Option<String>,
    pub snapshot: Option<Json<Snapshot>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct Snapshot {
    pub recommendations: Vec<Recommendation>,
    pub reference_list: Vec<super::ReferenceWork>,
}

pub(super) async fn view(pool: &PgPool, id: Uuid) -> Result<Option<StateRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT COALESCE(r.status, CASE WHEN s.status = 'ready' THEN 'interrupted'
                        ELSE 'waiting_for_summary' END) AS status,
                r.started_at + interval '30 seconds' AS retry_at, r.error_code, r.snapshot
         FROM conversations c
         LEFT JOIN book_recommendations r ON r.conversation_id = c.id
         LEFT JOIN summaries s ON s.conversation_id = c.id
         WHERE c.id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub(super) async fn references<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    activity: Option<Uuid>,
    topic: Option<Uuid>,
) -> Result<Vec<BookWork>, sqlx::Error> {
    match (activity, topic) {
        (Some(activity), Some(topic)) => {
            book_store::activity_books(executor, activity, topic).await
        }
        (None, Some(topic)) => book_store::topic_books(executor, topic).await,
        _ => Ok(Vec::new()),
    }
}

pub(super) async fn shared(
    pool: &PgPool,
    exclude: &[Uuid],
    language: &str,
) -> Result<Vec<BookWork>, sqlx::Error> {
    let (aliases, prefix) = selection::language_codes(language);
    sqlx::query_as(
        "SELECT w.*, e.editions
         FROM book_works w
         CROSS JOIN LATERAL (
             SELECT jsonb_agg(to_jsonb(b) ORDER BY b.created_at, b.id) AS editions
             FROM books b
             CROSS JOIN LATERAL (SELECT lower(replace(btrim(b.language), '_', '-')) AS lang) codes
             WHERE b.work_id = w.id AND b.is_active
               AND (lang = ANY($3) OR starts_with(lang, $4))
         ) e
         WHERE w.is_active AND NOT (w.id = ANY($1)) AND e.editions IS NOT NULL
         ORDER BY w.created_at DESC, w.id LIMIT $2",
    )
    .bind(exclude)
    .bind(selection::MAX_WORKS as i64)
    .bind(aliases)
    .bind(prefix)
    .fetch_all(pool)
    .await
}

fn version(language: &str) -> String {
    format!("{}/{language}", super::RULES_VERSION)
}

pub(super) async fn claim_automatic(
    transaction: &mut Transaction<'_, Postgres>,
    id: Uuid,
    language: &str,
) -> Result<bool, sqlx::Error> {
    // A savepoint keeps bookkeeping failures from affecting the summary transaction.
    let mut tx = transaction.begin().await?;
    let new = sqlx::query(
        "INSERT INTO book_recommendations (conversation_id, status, rules_version)
         VALUES ($1, 'pending', $2) ON CONFLICT DO NOTHING",
    )
    .bind(id)
    .bind(version(language))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(new.rows_affected() == 1)
}

pub(super) async fn claim_retry(
    conn: &mut PgConnection,
    id: Uuid,
    language: &str,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO book_recommendations (conversation_id, status, rules_version)
         SELECT $1, 'pending', $2
         FROM summaries WHERE conversation_id = $1 AND status = 'ready'
         ON CONFLICT (conversation_id) DO UPDATE SET status = 'pending', error_code = NULL,
             attempt = book_recommendations.attempt + 1, started_at = clock_timestamp(), rules_version = $2
         WHERE book_recommendations.status IN ('failed', 'interrupted')
             AND book_recommendations.started_at <= clock_timestamp() - interval '30 seconds'
         RETURNING attempt",
    ).bind(id).bind(version(language)).fetch_optional(conn).await
}

pub(super) async fn pending(pool: &PgPool, id: Uuid, attempt: i64) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM book_recommendations
         WHERE conversation_id = $1 AND attempt = $2 AND status = 'pending')",
    )
    .bind(id)
    .bind(attempt)
    .fetch_one(pool)
    .await
}

pub(super) async fn finish(
    conn: &mut PgConnection,
    id: Uuid,
    attempt: i64,
    status: RecommendationStatus,
    error_code: Option<&str>,
    snapshot: Option<Snapshot>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE book_recommendations SET status = $3, error_code = $4, snapshot = $5
         WHERE conversation_id = $1 AND attempt = $2 AND status = 'pending'",
    )
    .bind(id)
    .bind(attempt)
    .bind(status)
    .bind(error_code)
    .bind(snapshot.map(Json))
    .execute(conn)
    .await?;
    Ok(())
}
