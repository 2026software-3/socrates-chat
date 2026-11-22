//! F-14.1 post-summary reading recommendations and student-only snapshots.
mod selection;
mod store;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{StatusCode, header},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    AppState,
    auth::RequireStudent,
    book_sources::BookCandidate,
    book_store::{self, Book, BookSourceRef, BookWork},
    chat::owned_conversation,
    diagnostics,
    error::{ApiError, db_error_class},
};
use selection::{Candidate, INVALID_OUTPUT};
use store::Snapshot;

const RULES_VERSION: &str = "book-recommendations-v2";
const INTERRUPTED: &str = "recommendation_interrupted";
const BOOKKEEPING_FAILED: &str = "recommendation_unavailable";

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/conversations/{id}/recommendations", get(read))
        .route("/api/conversations/{id}/recommendations/retry", post(retry))
}

#[derive(Clone, Copy, Serialize, sqlx::Type, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
enum RecommendationStatus {
    WaitingForSummary,
    Pending,
    Ready,
    NoResults,
    Failed,
    Interrupted,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
enum RecommendationCategory {
    TeacherList,
    GoogleBooks,
    OpenLibrary,
}

#[derive(Clone, Serialize, Deserialize, utoipa::ToSchema)]
struct Edition {
    book_id: Option<Uuid>,
    title: String,
    authors: Option<String>,
    language: Option<String>,
    publication_year: Option<String>,
    isbn: Option<String>,
    source_url: Option<String>,
    external_sources: Vec<BookSourceRef>,
    #[serde(skip)]
    #[schema(ignore)]
    description: Option<String>,
    #[serde(skip)]
    #[schema(ignore)]
    teacher_note: Option<String>,
}

impl From<Book> for Edition {
    fn from(book: Book) -> Self {
        Self {
            book_id: Some(book.id),
            title: book.title,
            authors: book.authors,
            language: book.language,
            publication_year: book.publication_year,
            isbn: book.isbn,
            source_url: book.source_url,
            external_sources: book.external_sources,
            description: book.description,
            teacher_note: book.recommendation_note,
        }
    }
}

impl Edition {
    fn from_external(book: BookCandidate, category: RecommendationCategory) -> Option<Self> {
        let expected_source = match category {
            RecommendationCategory::GoogleBooks => "Google Books",
            RecommendationCategory::OpenLibrary => "Open Library",
            RecommendationCategory::TeacherList => return None,
        };
        if book.source != expected_source
            || book.source_id.trim().is_empty()
            || book.title.trim().is_empty()
        {
            return None;
        }
        Some(Self {
            book_id: None,
            title: book.title,
            authors: book.authors,
            language: book.language,
            publication_year: book.publication_year,
            isbn: book.isbn,
            source_url: book.source_url,
            external_sources: vec![BookSourceRef {
                source: book.source,
                source_id: book.source_id,
            }],
            description: book.description,
            teacher_note: None,
        })
    }
}

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
struct Recommendation {
    work_id: Option<Uuid>,
    source: RecommendationCategory,
    teacher_recommended: bool,
    editions: Vec<Edition>,
    reason: String,
    question: String,
}

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
struct ReferenceWork {
    work_id: Uuid,
    source: RecommendationCategory,
    editions: Vec<Edition>,
}

fn reference_list(works: Vec<BookWork>) -> Vec<ReferenceWork> {
    works
        .into_iter()
        .filter(|work| work.is_active)
        .filter_map(|work| {
            let editions: Vec<_> = work
                .editions
                .into_iter()
                .filter(|book| book.is_active)
                .map(Edition::from)
                .collect();
            (!editions.is_empty()).then_some(ReferenceWork {
                work_id: work.id,
                source: RecommendationCategory::TeacherList,
                editions,
            })
        })
        .collect()
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct RecommendationView {
    status: RecommendationStatus,
    recommendations: Vec<Recommendation>,
    teacher_reference_list: Vec<ReferenceWork>,
    error_code: Option<String>,
    /// Earliest retry time after a failed or interrupted attempt.
    retry_at: Option<DateTime<Utc>>,
    /// The result is unfinished and the student is eligible to request a retry.
    /// If retry_at is in the future, wait until that time.
    can_retry: bool,
}

impl RecommendationView {
    fn with_status(status: RecommendationStatus) -> Self {
        Self {
            status,
            recommendations: Vec::new(),
            teacher_reference_list: Vec::new(),
            error_code: None,
            retry_at: None,
            can_retry: false,
        }
    }
}

#[utoipa::path(
    get, path = "/api/conversations/{id}/recommendations", tag = "conversations",
    summary = "Read a student's post-summary book recommendations",
    params(("id" = Uuid, Path)),
    responses((status = 200, body = RecommendationView), crate::error::CommonErrors)
)]
pub(crate) async fn read(
    RequireStudent(user): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<
    (
        [(header::HeaderName, &'static str); 1],
        Json<RecommendationView>,
    ),
    ApiError,
> {
    if user.roles.can_teach() {
        return Err(ApiError::forbidden("forbidden", "只提供給學生本人"));
    }
    let conversation = owned_conversation(&state, user.user.id, id).await?;
    let row = store::view(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let mut view = RecommendationView::with_status(row.status);
    view.error_code = row.error_code;
    if let Some(snapshot) = row.snapshot {
        view.recommendations = snapshot.0.recommendations;
        view.teacher_reference_list = snapshot.0.reference_list;
    }
    if matches!(
        view.status,
        RecommendationStatus::Failed | RecommendationStatus::Interrupted
    ) {
        view.retry_at = row.retry_at;
    }
    if matches!(
        view.status,
        RecommendationStatus::Failed | RecommendationStatus::Interrupted
    ) {
        let mut conn = state.pool.acquire().await?;
        view.can_retry = store::eligible(&mut conn, id).await?;
        drop(conn);
        if view.status == RecommendationStatus::Interrupted {
            view.error_code = Some(INTERRUPTED.into());
        }
        view.teacher_reference_list = reference_list(
            store::references(&state.pool, conversation.activity_id, conversation.topic_id).await?,
        );
    }
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(view)))
}

#[utoipa::path(
    post, path = "/api/conversations/{id}/recommendations/retry", tag = "conversations",
    summary = "Manually retry a failed or interrupted book recommendation",
    params(("id" = Uuid, Path)),
    responses((status = 202, body = RecommendationView),
        (status = 409, description = "recommendation_not_retryable", body = crate::error::ErrorBody),
        (status = 429, description = "recommendation_retry_cooldown", body = crate::error::ErrorBody),
        crate::error::CommonErrors)
)]
pub(crate) async fn retry(
    RequireStudent(user): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<
    (
        StatusCode,
        [(header::HeaderName, &'static str); 1],
        Json<RecommendationView>,
    ),
    ApiError,
> {
    if user.roles.can_teach() {
        return Err(ApiError::forbidden("forbidden", "只提供給學生本人"));
    }
    let conversation = owned_conversation(&state, user.user.id, id).await?;
    let mut tx = state.pool.begin().await?;
    if store::eligible(&mut tx, id).await? {
        let attempt = store::claim_retry(&mut tx, id, &conversation.language).await?;
        tx.commit().await?;
        if let Some(attempt) = attempt {
            spawn(state, id, attempt);
            return Ok((
                StatusCode::ACCEPTED,
                [(header::CACHE_CONTROL, "no-store")],
                Json(RecommendationView::with_status(
                    RecommendationStatus::Pending,
                )),
            ));
        }
        if store::view(&state.pool, id).await?.is_some_and(|row| {
            matches!(
                row.status,
                RecommendationStatus::Failed | RecommendationStatus::Interrupted
            ) && row.retry_at.is_some_and(|retry_at| retry_at > Utc::now())
        }) {
            return Err(ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "recommendation_retry_cooldown",
                "請稍後再試",
            ));
        }
    }
    Err(ApiError::conflict(
        "recommendation_not_retryable",
        "目前無法重新產生推薦",
    ))
}

pub(crate) async fn claim_after_summary(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    language: &str,
) -> bool {
    match store::claim_automatic(tx, id, language).await {
        Ok(claimed) => claimed,
        Err(error) => {
            tracing::error!(class = %db_error_class(&error), "recommendation claim failed");
            false
        }
    }
}

pub(crate) fn spawn(state: AppState, id: Uuid, attempt: i64) {
    use tracing::Instrument;
    tokio::spawn(diagnostics::inherit_request(
        async move {
            if let Err(error) = run(&state, id, attempt).await {
                tracing::error!(class = %db_error_class(&error), "recommendation job failed");
                if let Ok(mut conn) = state.pool.acquire().await {
                    let _ = store::finish(
                        &mut conn,
                        id,
                        attempt,
                        RecommendationStatus::Failed,
                        Some(BOOKKEEPING_FAILED),
                        None,
                    )
                    .await;
                }
            }
        }
        .in_current_span(),
    ));
}

/// Recover interrupted jobs when the backend starts. Completed snapshots stay immutable.
pub async fn recover_orphaned(pool: &PgPool) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query(
        "UPDATE book_recommendations SET status = 'interrupted' WHERE status = 'pending'",
    )
    .execute(pool)
    .await?
    .rows_affected())
}

async fn run(state: &AppState, id: Uuid, attempt: i64) -> Result<(), sqlx::Error> {
    let Some(source) = store::source(&state.pool, id).await? else {
        return Ok(());
    };
    if !source.eligible {
        return store::finish(
            &mut *state.pool.acquire().await?,
            id,
            attempt,
            RecommendationStatus::Interrupted,
            None,
            None,
        )
        .await;
    }
    if !store::pending(&state.pool, id, attempt).await? {
        return Ok(());
    }
    let reference = store::references(&state.pool, source.activity_id, source.topic_id).await?;
    let ids: Vec<_> = reference.iter().map(|work| work.id).collect();
    let mut candidates: Vec<_> = reference
        .iter()
        .cloned()
        .filter_map(|work| Candidate::from_work(work, true, &source.language))
        .take(selection::MAX_WORKS)
        .collect();
    if candidates.len() < selection::MAX_WORKS {
        candidates.extend(
            store::shared(&state.pool, &ids, &source.language)
                .await?
                .into_iter()
                .filter_map(|work| Candidate::from_work(work, false, &source.language))
                .take(selection::MAX_WORKS - candidates.len()),
        );
    }
    let turns = crate::summary::load_messages_for(state, id).await?;
    let mut poll = tokio::time::interval(std::time::Duration::from_secs(1));
    let mut generation = std::pin::pin!(selection::generate(state, &source, &turns, &candidates));
    let result = loop {
        tokio::select! {
            biased;
            _ = poll.tick() => {
                if !store::pending(&state.pool, id, attempt).await? {
                    return Ok(());
                }
            }
            result = &mut generation => break result,
        }
    };
    let mut tx = state.pool.begin().await?;
    if !store::eligible(&mut tx, id).await? {
        store::finish(
            &mut tx,
            id,
            attempt,
            RecommendationStatus::Interrupted,
            None,
            None,
        )
        .await?;
    } else {
        let (status, code, snapshot) = match result {
            Ok(mut books) => {
                book_store::lock_catalog(&mut tx).await?;
                let had_results = !books.is_empty();
                recheck(&mut tx, &mut books).await?;
                if had_results && books.is_empty() {
                    (RecommendationStatus::Failed, Some(INVALID_OUTPUT), None)
                } else {
                    let status = if books.is_empty() {
                        RecommendationStatus::NoResults
                    } else {
                        RecommendationStatus::Ready
                    };
                    let reference_list = if books.is_empty() {
                        reference_list(
                            store::references(&mut *tx, source.activity_id, source.topic_id)
                                .await?,
                        )
                    } else {
                        Vec::new()
                    };
                    (
                        status,
                        None,
                        Some(Snapshot {
                            recommendations: books,
                            reference_list,
                        }),
                    )
                }
            }
            Err(code) => (RecommendationStatus::Failed, Some(code), None),
        };
        store::finish(&mut tx, id, attempt, status, code, snapshot).await?;
    }
    tx.commit().await
}

async fn recheck(
    conn: &mut sqlx::PgConnection,
    books: &mut Vec<Recommendation>,
) -> Result<(), sqlx::Error> {
    let versions: Vec<_> = books
        .iter()
        .flat_map(|work| work.editions.iter().filter_map(|edition| edition.book_id))
        .collect();
    let active: std::collections::HashSet<(Uuid, Uuid)> = sqlx::query_as::<_, (Uuid, Uuid)>(
        "SELECT b.id, b.work_id FROM books b JOIN book_works w ON w.id = b.work_id
         WHERE b.id = ANY($1) AND b.is_active AND w.is_active",
    )
    .bind(versions)
    .fetch_all(conn)
    .await?
    .into_iter()
    .collect();
    books.retain_mut(|work| {
        if let Some(id) = work.work_id {
            work.editions.retain(|edition| {
                edition
                    .book_id
                    .is_some_and(|version| active.contains(&(version, id)))
            });
        }
        !work.editions.is_empty()
    });
    Ok(())
}
