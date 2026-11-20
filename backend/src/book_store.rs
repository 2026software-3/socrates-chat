//! F-14.3 shared catalog and independent topic/activity reading lists.
//!
//! The HTTP layer authorizes staff and locks the owning topic/activity before replacing a list.
//! Mutations use the caller's transaction and catalog lock before owner/work/edition locks.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

/// Provider provenance survives metadata enrichment and manual corrections.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash, utoipa::ToSchema)]
pub struct BookSourceRef {
    pub source: String,
    pub source_id: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, utoipa::ToSchema)]
pub struct BookData {
    #[schema(min_length = 1, max_length = 255)]
    pub title: String,
    #[schema(max_length = 1000)]
    pub authors: Option<String>,
    pub language: Option<String>,
    pub publication_year: Option<String>,
    pub isbn: Option<String>,
    #[schema(max_length = 8192)]
    pub source_url: Option<String>,
    #[schema(max_length = 5000)]
    pub description: Option<String>,
    #[schema(max_length = 2000)]
    pub recommendation_note: Option<String>,
    #[serde(default)]
    pub external_sources: Vec<BookSourceRef>,
}

#[derive(Debug, Clone, Deserialize, Serialize, FromRow, utoipa::ToSchema)]
pub struct Book {
    pub id: Uuid,
    pub work_id: Uuid,
    pub title: String,
    pub authors: Option<String>,
    pub language: Option<String>,
    pub publication_year: Option<String>,
    pub isbn: Option<String>,
    pub source_url: Option<String>,
    pub description: Option<String>,
    pub recommendation_note: Option<String>,
    #[sqlx(json)]
    pub external_sources: Vec<BookSourceRef>,
    pub created_by: Option<Uuid>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn list(
    pool: &PgPool,
    query: Option<&str>,
    after: Option<Uuid>,
    limit: i64,
) -> Result<Vec<Book>, sqlx::Error> {
    sqlx::query_as(
        "SELECT * FROM books WHERE ($1::text IS NULL
           OR strpos(lower(title), lower($1)) > 0
           OR strpos(lower(COALESCE(authors, '')), lower($1)) > 0)
           AND ($2::uuid IS NULL OR id > $2) ORDER BY id LIMIT $3",
    )
    .bind(query)
    .bind(after)
    .bind(limit)
    .fetch_all(pool)
    .await
}

pub async fn get<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    id: Uuid,
) -> Result<Option<Book>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM books WHERE id = $1")
        .bind(id)
        .fetch_optional(executor)
        .await
}

/// Input is normalized and validated by the service layer. `None` owner is a team CSV import.
pub async fn create<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    created_by: Option<Uuid>,
    data: &BookData,
) -> Result<Book, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO books (title, authors, language, publication_year, isbn, source_url,
                            description, recommendation_note, created_by, external_sources)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING *",
    )
    .bind(&data.title)
    .bind(&data.authors)
    .bind(&data.language)
    .bind(&data.publication_year)
    .bind(&data.isbn)
    .bind(&data.source_url)
    .bind(&data.description)
    .bind(&data.recommendation_note)
    .bind(created_by)
    .bind(sqlx::types::Json(&data.external_sources))
    .fetch_one(executor)
    .await
}

pub async fn update_owned<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    id: Uuid,
    actor_id: Uuid,
    is_admin: bool,
    data: &BookData,
) -> Result<Option<Book>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE books SET title=$4, authors=$5, language=$6, publication_year=$7,
           isbn=$8, source_url=$9, description=$10, recommendation_note=$11,
           external_sources=$12, updated_at=now()
         WHERE id=$1 AND ($3 OR created_by=$2) RETURNING *",
    )
    .bind(id)
    .bind(actor_id)
    .bind(is_admin)
    .bind(&data.title)
    .bind(&data.authors)
    .bind(&data.language)
    .bind(&data.publication_year)
    .bind(&data.isbn)
    .bind(&data.source_url)
    .bind(&data.description)
    .bind(&data.recommendation_note)
    .bind(sqlx::types::Json(&data.external_sources))
    .fetch_optional(executor)
    .await
}

/// Only the already-authorized administrator may call this operation.
pub async fn set_active<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    id: Uuid,
    active: bool,
) -> Result<Option<Book>, sqlx::Error> {
    sqlx::query_as("UPDATE books SET is_active=$2, updated_at=now() WHERE id=$1 RETURNING *")
        .bind(id)
        .bind(active)
        .fetch_optional(executor)
        .await
}

/// Only the already-authorized administrator may delete; associations cascade, not the converse.
pub async fn delete<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query("DELETE FROM books WHERE id=$1")
        .bind(id)
        .execute(executor)
        .await?
        .rows_affected()
        > 0)
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub struct BookImpact {
    pub deletes_work: bool,
    pub topic_count: i64,
    pub activity_topic_count: i64,
}

pub async fn impact<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    id: Uuid,
) -> Result<BookImpact, sqlx::Error> {
    sqlx::query_as(
        "WITH edition_work AS (SELECT work_id FROM books WHERE id=$1),
         last_edition AS (SELECT work_id FROM edition_work WHERE (SELECT count(*) FROM books b WHERE b.work_id=edition_work.work_id)=1)
         SELECT EXISTS(SELECT 1 FROM last_edition) AS deletes_work,
           (SELECT count(*) FROM topic_books WHERE book_id IN (SELECT work_id FROM last_edition)) AS topic_count,
           (SELECT count(*) FROM activity_topic_books WHERE book_id IN (SELECT work_id FROM last_edition)) AS activity_topic_count",
    )
    .bind(id)
    .fetch_one(executor)
    .await
}

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct BookWork {
    pub id: Uuid,
    pub title: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[sqlx(json)]
    pub editions: Vec<Book>,
}

// A single statement observes a consistent work and edition snapshot.

pub async fn list_works(pool: &PgPool, query: Option<&str>) -> Result<Vec<BookWork>, sqlx::Error> {
    sqlx::query_as("SELECT w.*, COALESCE((SELECT jsonb_agg(to_jsonb(b) ORDER BY b.created_at,b.id) FROM books b WHERE b.work_id=w.id),'[]'::jsonb) AS editions FROM book_works w WHERE ($1::text IS NULL OR strpos(lower(w.title),lower($1))>0 OR EXISTS (SELECT 1 FROM books b WHERE b.work_id=w.id AND (strpos(lower(b.title),lower($1))>0 OR strpos(lower(COALESCE(b.authors,'')),lower($1))>0))) ORDER BY w.created_at DESC,w.id")
        .bind(query).fetch_all(pool).await
}

pub async fn get_work<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    id: Uuid,
) -> Result<Option<BookWork>, sqlx::Error> {
    sqlx::query_as("SELECT w.*, COALESCE((SELECT jsonb_agg(to_jsonb(b) ORDER BY b.created_at,b.id) FROM books b WHERE b.work_id=w.id),'[]'::jsonb) AS editions FROM book_works w WHERE w.id=$1")
        .bind(id)
        .fetch_optional(executor)
        .await
}

pub async fn topic_books<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    topic_id: Uuid,
) -> Result<Vec<BookWork>, sqlx::Error> {
    sqlx::query_as("SELECT w.*, COALESCE((SELECT jsonb_agg(to_jsonb(b) ORDER BY b.created_at,b.id) FROM books b WHERE b.work_id=w.id),'[]'::jsonb) AS editions FROM book_works w JOIN topic_books tb ON tb.book_id=w.id WHERE tb.topic_id=$1 ORDER BY w.title,w.id")
        .bind(topic_id).fetch_all(executor).await
}

pub async fn activity_books<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    activity_id: Uuid,
    topic_id: Uuid,
) -> Result<Vec<BookWork>, sqlx::Error> {
    sqlx::query_as("SELECT w.*, COALESCE((SELECT jsonb_agg(to_jsonb(b) ORDER BY b.created_at,b.id) FROM books b WHERE b.work_id=w.id),'[]'::jsonb) AS editions FROM book_works w JOIN activity_topic_books ab ON ab.book_id=w.id WHERE ab.activity_id=$1 AND ab.topic_id=$2 ORDER BY w.title,w.id")
        .bind(activity_id).bind(topic_id).fetch_all(executor).await
}

/// Acquire before topic/activity/work/edition row locks in any catalog/list write transaction.
pub async fn lock_catalog(conn: &mut PgConnection) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(831403)")
        .execute(conn)
        .await?;
    Ok(())
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub struct WorkImpact {
    pub topic_count: i64,
    pub activity_topic_count: i64,
    pub edition_count: i64,
}

pub async fn work_impact<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    id: Uuid,
) -> Result<WorkImpact, sqlx::Error> {
    sqlx::query_as("SELECT (SELECT count(*) FROM topic_books WHERE book_id=$1) AS topic_count,(SELECT count(*) FROM activity_topic_books WHERE book_id=$1) AS activity_topic_count,(SELECT count(*) FROM books WHERE work_id=$1) AS edition_count")
        .bind(id).fetch_one(executor).await
}

/// Requires a transaction; the caller acquires the catalog lock before other row locks.
pub async fn set_work_active(
    conn: &mut PgConnection,
    id: Uuid,
    active: bool,
) -> Result<Option<BookWork>, sqlx::Error> {
    lock_catalog(conn).await?;
    sqlx::query("UPDATE book_works SET is_active=$2,updated_at=now() WHERE id=$1")
        .bind(id)
        .bind(active)
        .execute(&mut *conn)
        .await?;
    get_work(conn, id).await
}

/// Requires a transaction; deleting a work cascades editions and both list kinds.
pub async fn delete_work(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
    lock_catalog(conn).await?;
    Ok(sqlx::query("DELETE FROM book_works WHERE id=$1")
        .bind(id)
        .execute(conn)
        .await?
        .rows_affected()
        > 0)
}

/// Requires a transaction; preserve target status and all edition metadata while redirecting links.
pub async fn merge_works(
    conn: &mut PgConnection,
    source: Uuid,
    target: Uuid,
) -> Result<bool, sqlx::Error> {
    lock_catalog(conn).await?;
    if source == target {
        return Ok(false);
    }
    let ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM book_works WHERE id=ANY($1) ORDER BY id FOR UPDATE")
            .bind(vec![source, target])
            .fetch_all(&mut *conn)
            .await?;
    if ids.len() != 2 {
        return Ok(false);
    }
    sqlx::query("INSERT INTO topic_books(topic_id,book_id) SELECT topic_id,$2 FROM topic_books WHERE book_id=$1 ON CONFLICT DO NOTHING").bind(source).bind(target).execute(&mut *conn).await?;
    sqlx::query("INSERT INTO activity_topic_books(activity_id,topic_id,book_id) SELECT activity_id,topic_id,$2 FROM activity_topic_books WHERE book_id=$1 ON CONFLICT DO NOTHING").bind(source).bind(target).execute(&mut *conn).await?;
    sqlx::query("UPDATE books SET work_id=$2,updated_at=now() WHERE work_id=$1")
        .bind(source)
        .bind(target)
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE book_works SET updated_at=now() WHERE id=$1")
        .bind(target)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM book_works WHERE id=$1")
        .bind(source)
        .execute(conn)
        .await?;
    Ok(true)
}

/// Requires a transaction; changing ownership preserves source work/list identity, even if empty.
pub async fn move_edition(
    conn: &mut PgConnection,
    edition: Uuid,
    target: Uuid,
) -> Result<Option<Book>, sqlx::Error> {
    lock_catalog(conn).await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM book_works WHERE id=$1)")
        .bind(target)
        .fetch_one(&mut *conn)
        .await?;
    if !exists {
        return Ok(None);
    }
    sqlx::query_as("UPDATE books SET work_id=$2,updated_at=now() WHERE id=$1 RETURNING *")
        .bind(edition)
        .bind(target)
        .fetch_optional(conn)
        .await
}

async fn selectable(
    conn: &mut PgConnection,
    ids: &[Uuid],
    existing: &[Uuid],
) -> Result<bool, sqlx::Error> {
    let books: Vec<(Uuid, bool)> =
        sqlx::query_as("SELECT id, (is_active AND EXISTS (SELECT 1 FROM books b WHERE b.work_id=book_works.id AND b.is_active)) FROM book_works WHERE id=ANY($1) ORDER BY id FOR SHARE")
            .bind(ids)
            .fetch_all(conn)
            .await?;
    Ok(books.len() == ids.len()
        && books
            .iter()
            .all(|(id, active)| *active || existing.contains(id)))
}

/// Requires an enclosing transaction and a lock on the topic. False leaves the list unchanged.
pub async fn replace_topic_books(
    conn: &mut PgConnection,
    topic_id: Uuid,
    book_ids: &[Uuid],
) -> Result<bool, sqlx::Error> {
    lock_catalog(conn).await?;
    let current: Vec<Uuid> =
        sqlx::query_scalar("SELECT book_id FROM topic_books WHERE topic_id=$1")
            .bind(topic_id)
            .fetch_all(&mut *conn)
            .await?;
    if !selectable(conn, book_ids, &current).await? {
        return Ok(false);
    }
    sqlx::query("DELETE FROM topic_books WHERE topic_id=$1 AND NOT(book_id=ANY($2))")
        .bind(topic_id)
        .bind(book_ids)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "INSERT INTO topic_books(topic_id,book_id) SELECT $1, unnest($2::uuid[])
         ON CONFLICT DO NOTHING",
    )
    .bind(topic_id)
    .bind(book_ids)
    .execute(conn)
    .await?;
    Ok(true)
}

/// Requires an enclosing transaction and a lock on the activity. False leaves the list unchanged.
pub async fn replace_activity_books(
    conn: &mut PgConnection,
    activity_id: Uuid,
    topic_id: Uuid,
    book_ids: &[Uuid],
) -> Result<bool, sqlx::Error> {
    lock_catalog(conn).await?;
    let current: Vec<Uuid> = sqlx::query_scalar(
        "SELECT book_id FROM activity_topic_books WHERE activity_id=$1 AND topic_id=$2",
    )
    .bind(activity_id)
    .bind(topic_id)
    .fetch_all(&mut *conn)
    .await?;
    if !selectable(conn, book_ids, &current).await? {
        return Ok(false);
    }
    sqlx::query(
        "DELETE FROM activity_topic_books WHERE activity_id=$1 AND topic_id=$2
         AND NOT(book_id=ANY($3))",
    )
    .bind(activity_id)
    .bind(topic_id)
    .bind(book_ids)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO activity_topic_books(activity_id,topic_id,book_id)
         SELECT $1,$2,unnest($3::uuid[]) ON CONFLICT DO NOTHING",
    )
    .bind(activity_id)
    .bind(topic_id)
    .bind(book_ids)
    .execute(conn)
    .await?;
    Ok(true)
}

/// Keep unchanged topic rows (and their independently edited book lists) during reorder.
/// Called inside the activity update transaction, after authorization and conversation checks.
pub async fn sync_activity_topics(
    conn: &mut PgConnection,
    activity_id: Uuid,
    topic_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    lock_catalog(conn).await?;
    sqlx::query("SET CONSTRAINTS activity_topics_position_unique DEFERRED")
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM activity_topics WHERE activity_id=$1 AND NOT(topic_id=ANY($2))")
        .bind(activity_id)
        .bind(topic_ids)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "UPDATE activity_topics at SET position=(t.ord-1)::integer
         FROM unnest($2::uuid[]) WITH ORDINALITY AS t(topic_id,ord)
         WHERE at.activity_id=$1 AND at.topic_id=t.topic_id",
    )
    .bind(activity_id)
    .bind(topic_ids)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO activity_topics(activity_id,topic_id,position)
         SELECT $1,t.topic_id,(t.ord-1)::integer
         FROM unnest($2::uuid[]) WITH ORDINALITY AS t(topic_id,ord)
         WHERE NOT EXISTS (SELECT 1 FROM activity_topics at
                           WHERE at.activity_id=$1 AND at.topic_id=t.topic_id)",
    )
    .bind(activity_id)
    .bind(topic_ids)
    .execute(conn)
    .await?;
    Ok(())
}
