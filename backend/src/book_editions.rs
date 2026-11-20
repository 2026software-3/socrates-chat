//! Other-edition suggestions are read-only until a teacher confirms the import.
use crate::{
    AppState,
    accounts::{confirmation, ensure_confirmed},
    auth::RequireTeacher,
    book_sources::RelatedEdition,
    book_store::{self, BookData, BookWork},
    error::{ApiError, ErrorBody},
};
use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
    routing::{get, post},
};
use serde::Deserialize;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/books/{id}/editions", get(related))
        .route("/api/book-works/{id}/editions", post(import))
}

#[utoipa::path(get,path="/api/books/{id}/editions",tag="books",summary="搜尋其他語言／出版版本候選；不寫入書庫",params(("id"=Uuid,Path)),responses((status=200,description="Open Library 作品關係候選，仍須教師確認；有界查詢不保證找齊",body=Vec<RelatedEdition>),(status=503,description="book_source_unavailable",body=ErrorBody),crate::error::CommonErrors))]
pub(crate) async fn related(
    _: RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<RelatedEdition>>, ApiError> {
    let book = book_store::get(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let mut candidates = state.book_source.related(&book).await.map_err(|_| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "book_source_unavailable",
            "外部服務暫時無法使用",
        )
    })?;
    let known = book_store::list(&state.pool, None, None, i64::MAX).await?;
    candidates.retain(|r| {
        !known.iter().any(|b| {
            b.external_sources
                .iter()
                .any(|s| s.source == r.book.source && s.source_id == r.book.source_id)
                || r.book
                    .isbn
                    .as_ref()
                    .is_some_and(|isbn| b.isbn.as_ref() == Some(isbn))
        })
    });
    Ok(Json(candidates))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct ImportEditions {
    confirm_title: String,
    #[schema(min_items = 1, max_items = 20)]
    books: Vec<BookData>,
}
#[utoipa::path(post,path="/api/book-works/{id}/editions",tag="books",summary="教師確認後將選定版本加入既有作品；重複版本略過",params(("id"=Uuid,Path)),request_body=ImportEditions,responses((status=200,description="更新後作品；不自動移動其他作品內已有的版本",body=BookWork),(status=400,description="confirmation_mismatch / invalid_book / invalid_book_*",body=ErrorBody),crate::error::CommonErrors))]
pub(crate) async fn import(
    RequireTeacher(user): RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    body: Result<Json<ImportEditions>, JsonRejection>,
) -> Result<Json<BookWork>, ApiError> {
    let data = confirmation(body)?;
    if data.books.is_empty() || data.books.len() > 20 {
        return Err(ApiError::bad_request(
            "invalid_book",
            "請選擇一至二十個版本",
        ));
    }
    let books = data
        .books
        .into_iter()
        .map(crate::books::normalize_book)
        .collect::<Result<Vec<_>, _>>()?;
    let mut tx = state.pool.begin().await?;
    book_store::lock_catalog(&mut tx).await?;
    let work = book_store::get_work(&mut *tx, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    ensure_confirmed(&data.confirm_title, &work.title)?;
    for book in books {
        let duplicate:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM books b WHERE ($1::text IS NOT NULL AND b.isbn=$1) OR EXISTS(SELECT 1 FROM jsonb_array_elements($2::jsonb) r WHERE b.external_sources @> jsonb_build_array(r)))")
            .bind(&book.isbn).bind(sqlx::types::Json(&book.external_sources)).fetch_one(&mut *tx).await?;
        if !duplicate {
            book_store::create_in_work(&mut *tx, user.user.id, &book, id).await?;
        }
    }
    let work = book_store::get_work(&mut *tx, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    tx.commit().await?;
    Ok(Json(work))
}
