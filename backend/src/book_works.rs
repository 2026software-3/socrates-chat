//! Confirmed work grouping is separate from ownership of edition metadata.
use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::StatusCode,
    routing::{get, patch, post, put},
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    AppState,
    accounts::{confirmation, ensure_confirmed},
    auth::{RequireAdmin, RequireTeacher},
    book_store::{self, Book, BookWork, MergeWorkError, WorkImpact},
    error::{ApiError, ErrorBody},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/book-works", get(list))
        .route(
            "/api/book-works/{id}",
            axum::routing::delete(remove).patch(rename),
        )
        .route("/api/book-works/{id}/impact", get(impact))
        .route("/api/book-works/{id}/merge", post(merge))
        .route("/api/book-works/{id}/status", patch(set_status))
        .route("/api/books/{id}/work", put(move_edition))
        .route("/api/books/{id}/work-impact", get(edition_work_impact))
}

#[derive(Default, Deserialize)]
pub(crate) struct SearchQuery {
    q: Option<String>,
}

#[utoipa::path(get, path="/api/book-works", tag="books", summary="共用作品與各版本（含停用及空作品）", params(("q"=Option<String>, Query, description="作品、版本書名或作者；最多1000字元")), responses((status=200, description="作品與全部版本", body=Vec<BookWork>), (status=400, description="invalid_filter", body=ErrorBody), crate::error::CommonErrors))]
pub(crate) async fn list(
    _: RequireTeacher,
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<BookWork>>, ApiError> {
    let q = query.q.as_deref().map(str::trim);
    if q.is_some_and(|q| q.is_empty() || q.chars().count() > 1000) {
        return Err(ApiError::invalid_filter());
    }
    Ok(Json(book_store::list_works(&state.pool, q).await?))
}

#[utoipa::path(get, path="/api/book-works/{id}/impact", tag="books", summary="教師確認作品合併／管理者刪除影響", params(("id"=Uuid, Path)), responses((status=200, description="版本、題目與活動題目書單數", body=WorkImpact), crate::error::CommonErrors))]
pub(crate) async fn impact(
    _: RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<WorkImpact>, ApiError> {
    let mut tx = state.pool.begin().await?;
    // A consistent preview; mutations re-check the source and confirmation in their transaction.
    book_store::lock_catalog(&mut tx).await?;
    book_store::get_work(&mut *tx, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let impact = book_store::work_impact(&mut *tx, id).await?;
    tx.commit().await?;
    Ok(Json(impact))
}

#[utoipa::path(get, path="/api/books/{id}/work-impact", tag="books", summary="移動版本前確認來源作品影響（最後一版移動仍保留來源作品及書單）", params(("id"=Uuid, Path)), responses((status=200, description="來源作品版本及書單數", body=WorkImpact), crate::error::CommonErrors))]
pub(crate) async fn edition_work_impact(
    _: RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<WorkImpact>, ApiError> {
    let mut tx = state.pool.begin().await?;
    book_store::lock_catalog(&mut tx).await?;
    let edition = book_store::get(&mut *tx, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let impact = book_store::work_impact(&mut *tx, edition.work_id).await?;
    tx.commit().await?;
    Ok(Json(impact))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct MergeWork {
    target_work_id: Uuid,
    confirm_title: String,
}

#[utoipa::path(post, path="/api/book-works/{id}/merge", tag="books", summary="教師確認合併作品；書單轉向保留作品並去重", params(("id"=Uuid, Path)), request_body=MergeWork, responses((status=200, description="保留作品；不改變版本內容編輯權限及啟用狀態", body=BookWork), (status=400, description="confirmation_mismatch / invalid_book_work", body=ErrorBody), (status=409, description="book_work_inactive / book_work_archived_list", body=ErrorBody), crate::error::CommonErrors))]
pub(crate) async fn merge(
    _: RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    body: Result<Json<MergeWork>, JsonRejection>,
) -> Result<Json<BookWork>, ApiError> {
    let mut tx = state.pool.begin().await?;
    book_store::lock_catalog(&mut tx).await?;
    let source = book_store::get_work(&mut *tx, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let data = confirmation(body)?;
    ensure_confirmed(&data.confirm_title, &source.title)?;
    if id == data.target_work_id {
        return Err(ApiError::bad_request(
            "invalid_book_work",
            "來源與目的作品不可相同",
        ));
    }
    ensure_active_source(source.is_active)?;
    match book_store::merge_works(&mut tx, id, data.target_work_id).await {
        Ok(true) => {}
        Ok(false) => return Err(ApiError::not_found()),
        Err(MergeWorkError::ArchivedList) => {
            return Err(ApiError::conflict(
                "book_work_archived_list",
                "來源作品已列入封存活動或封存教室的書單，無法合併。",
            ));
        }
        Err(MergeWorkError::Database(error)) => return Err(error.into()),
    }
    let target = book_store::get_work(&mut *tx, data.target_work_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    tx.commit().await?;
    Ok(Json(target))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct MoveEdition {
    work_id: Uuid,
    confirm_title: String,
}

#[utoipa::path(put, path="/api/books/{id}/work", tag="books", summary="教師確認移動版本的作品歸屬；不自動新增目的作品書單關聯", params(("id"=Uuid, Path)), request_body=MoveEdition, responses((status=200, description="移動後版本；保留來源作品及書單", body=Book), (status=400, description="confirmation_mismatch / invalid_book_work", body=ErrorBody), (status=409, description="book_work_inactive", body=ErrorBody), crate::error::CommonErrors))]
pub(crate) async fn move_edition(
    _: RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    body: Result<Json<MoveEdition>, JsonRejection>,
) -> Result<Json<Book>, ApiError> {
    let mut tx = state.pool.begin().await?;
    book_store::lock_catalog(&mut tx).await?;
    let old = book_store::get(&mut *tx, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let data = confirmation(body)?;
    ensure_confirmed(&data.confirm_title, &old.title)?;
    if old.work_id == data.work_id {
        return Err(ApiError::bad_request(
            "invalid_book_work",
            "來源與目的作品不可相同",
        ));
    }
    let source_is_active = book_store::work_is_active(&mut *tx, old.work_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    ensure_active_source(source_is_active)?;
    let book = book_store::move_edition(&mut tx, id, data.work_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    tx.commit().await?;
    Ok(Json(book))
}

// Call under the catalog lock shared with work status updates.
fn ensure_active_source(is_active: bool) -> Result<(), ApiError> {
    if !is_active {
        return Err(ApiError::conflict(
            "book_work_inactive",
            "來源作品已停用，請先由管理者重新啟用",
        ));
    }
    Ok(())
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct WorkStatus {
    is_active: bool,
}

#[utoipa::path(patch, path="/api/book-works/{id}/status", tag="books", summary="管理者停用或重新啟用整個作品；版本狀態獨立保留", params(("id"=Uuid, Path)), request_body=WorkStatus, responses((status=200, description="更新後作品", body=BookWork), crate::error::CommonErrors))]
pub(crate) async fn set_status(
    _: RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(data): Json<WorkStatus>,
) -> Result<Json<BookWork>, ApiError> {
    let mut tx = state.pool.begin().await?;
    book_store::lock_catalog(&mut tx).await?;
    let work = book_store::set_work_active(&mut tx, id, data.is_active)
        .await?
        .ok_or_else(ApiError::not_found)?;
    tx.commit().await?;
    Ok(Json(work))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct DeleteWork {
    confirm_title: String,
}

#[utoipa::path(delete, path="/api/book-works/{id}", tag="books", summary="管理者確認永久刪除作品、全部版本及書單關聯", params(("id"=Uuid, Path)), request_body=DeleteWork, responses((status=204, description="已刪除；既有推薦快照不變"), (status=400, description="confirmation_mismatch", body=ErrorBody), crate::error::CommonErrors))]
pub(crate) async fn remove(
    _: RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    body: Result<Json<DeleteWork>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let mut tx = state.pool.begin().await?;
    book_store::lock_catalog(&mut tx).await?;
    let work = book_store::get_work(&mut *tx, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    ensure_confirmed(&confirmation(body)?.confirm_title, &work.title)?;
    book_store::delete_work(&mut tx, id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct RenameWork {
    #[schema(min_length = 1, max_length = 255)]
    title: String,
}
#[utoipa::path(patch,path="/api/book-works/{id}",tag="books",summary="教師或管理者修改共用作品名稱，不改版本書名或書單關聯",params(("id"=Uuid,Path)),request_body=RenameWork,responses((status=200,description="改名後作品",body=BookWork),(status=400,description="invalid_book_title",body=ErrorBody),crate::error::CommonErrors))]
pub(crate) async fn rename(
    _: RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    body: Result<Json<RenameWork>, JsonRejection>,
) -> Result<Json<BookWork>, ApiError> {
    let data = confirmation(body)?;
    let title = crate::books::normalize_book(crate::book_store::BookData {
        title: data.title,
        ..Default::default()
    })?
    .title;
    let mut tx = state.pool.begin().await?;
    book_store::lock_catalog(&mut tx).await?;
    let work = book_store::rename_work(&mut tx, id, &title)
        .await?
        .ok_or_else(ApiError::not_found)?;
    tx.commit().await?;
    Ok(Json(work))
}
