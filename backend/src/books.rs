//! F-14.3 教師共用書庫與題目／活動書單。個人推薦由 F-14.1 實作。
use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::StatusCode,
    routing::{get, patch},
};
use serde::Deserialize;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::{
    AppState,
    accounts::{confirmation, ensure_confirmed},
    auth::{CurrentUser, RequireAdmin, RequireTeacher},
    book_store::{self, Book, BookData, BookImpact, BookWork},
    error::{ApiError, ErrorBody},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/books", get(list).post(create))
        .route("/api/books/search", get(search))
        .route("/api/books/{id}", get(read).put(update).delete(remove))
        .route("/api/books/{id}/status", patch(set_status))
        .route("/api/books/{id}/deletion-impact", get(deletion_impact))
        .route(
            "/api/topics/{id}/books",
            get(topic_list).put(replace_topic_list),
        )
        .route(
            "/api/activities/{id}/topics/{topic_id}/books",
            get(activity_list).put(replace_activity_list),
        )
}

/// 同一個欄位驗證供 HTTP 與開發團隊 CSV 匯入使用，不讓匯入繞過規格上限。
pub fn normalize_book(mut data: BookData) -> Result<BookData, ApiError> {
    data.title = data.title.trim().to_owned();
    if data.title.is_empty() || data.title.chars().count() > 255 {
        return Err(ApiError::bad_request("invalid_book_title", "書名長度不符"));
    }
    fn optional(
        v: Option<String>,
        max: usize,
        code: &'static str,
    ) -> Result<Option<String>, ApiError> {
        let value = v.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
        if value.as_ref().is_some_and(|s| s.chars().count() > max) {
            return Err(ApiError::bad_request(code, "書目欄位長度不符"));
        }
        Ok(value)
    }
    data.authors = optional(data.authors, 1000, "invalid_book_authors")?;
    data.description = optional(data.description, 5000, "invalid_book_description")?;
    data.recommendation_note = optional(
        data.recommendation_note,
        2000,
        "invalid_book_recommendation_note",
    )?;
    data.source_url = optional(data.source_url, 8192, "invalid_book_source_url")?;
    if let Some(url) = &data.source_url {
        let valid = reqwest::Url::parse(url)
            .is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some());
        if !valid {
            return Err(ApiError::bad_request(
                "invalid_book_source_url",
                "來源連結格式不符",
            ));
        }
    }
    data.language = data
        .language
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    data.publication_year = data
        .publication_year
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    data.isbn = data
        .isbn
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .map(|s| normalize_isbn(&s))
        .transpose()?;
    let mut seen = std::collections::HashSet::new();
    for reference in &data.external_sources {
        let valid = match reference.source.as_str() {
            "Google Books" => {
                !reference.source_id.is_empty()
                    && reference
                        .source_id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
            }
            "Open Library" => reference
                .source_id
                .strip_prefix("/books/OL")
                .and_then(|id| id.strip_suffix('M'))
                .is_some_and(|digits| {
                    !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit())
                }),
            _ => false,
        };
        if !valid {
            return Err(ApiError::bad_request(
                "invalid_book_external_sources",
                "外部書目來源格式不符",
            ));
        }
    }
    data.external_sources
        .retain(|reference| seen.insert(reference.clone()));
    Ok(data)
}

/// ISBN-10 與 ISBN-13 都驗證檢查碼，儲存為 ISBN-13，方便版本比對。
pub fn normalize_isbn(raw: &str) -> Result<String, ApiError> {
    let s: String = raw
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    let invalid = || ApiError::bad_request("invalid_book_isbn", "ISBN 格式不符");
    let digits: Vec<u32> = s
        .chars()
        .enumerate()
        .map(|(i, c)| {
            if i == 9 && s.len() == 10 && matches!(c, 'X' | 'x') {
                Some(10)
            } else if c.is_ascii_digit() {
                c.to_digit(10)
            } else {
                None
            }
        })
        .collect::<Option<_>>()
        .ok_or_else(invalid)?;
    match digits.len() {
        10 if digits
            .iter()
            .enumerate()
            .map(|(i, d)| (10 - i as u32) * d)
            .sum::<u32>()
            % 11
            == 0 =>
        {
            let prefix = format!("978{}", &s[..9]);
            let sum: u32 = prefix
                .bytes()
                .enumerate()
                .map(|(i, b)| u32::from(b - b'0') * if i % 2 == 0 { 1 } else { 3 })
                .sum();
            Ok(format!("{prefix}{}", (10 - sum % 10) % 10))
        }
        13 if (s.starts_with("978") || s.starts_with("979"))
            && digits
                .iter()
                .enumerate()
                .map(|(i, d)| d * if i % 2 == 0 { 1 } else { 3 })
                .sum::<u32>()
                % 10
                == 0 =>
        {
            Ok(s)
        }
        _ => Err(invalid()),
    }
}

#[derive(Default, Deserialize)]
pub(crate) struct SearchQuery {
    q: Option<String>,
}
fn query_text(query: &SearchQuery, required: bool) -> Result<Option<&str>, ApiError> {
    match query.q.as_deref().map(str::trim) {
        Some(q) if q.is_empty() || q.chars().count() > 1000 => Err(ApiError::invalid_filter()),
        None if required => Err(ApiError::invalid_filter()),
        q => Ok(q),
    }
}

#[utoipa::path(get, path="/api/books", tag="books", summary="查詢共用書庫（教師／管理者）", params(("q"=Option<String>, Query, description="書名或作者子字串；最多 1000 字元")), responses((status=200, description="含啟用與停用書目", body=Vec<Book>), (status=400, description="invalid_filter", body=ErrorBody), crate::error::CommonErrors))]
pub(crate) async fn list(
    _: RequireTeacher,
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<Book>>, ApiError> {
    let q = query_text(&query, false)?;
    Ok(Json(
        book_store::list(&state.pool, q, None, i64::MAX).await?,
    ))
}

#[utoipa::path(get, path="/api/books/{id}", tag="books", summary="查看共用書目", params(("id"=Uuid, Path)), responses((status=200, description="書目", body=Book), crate::error::CommonErrors))]
pub(crate) async fn read(
    _: RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Book>, ApiError> {
    Ok(Json(
        book_store::get(&state.pool, id)
            .await?
            .ok_or_else(ApiError::not_found)?,
    ))
}

#[utoipa::path(post, path="/api/books", tag="books", summary="新增共用書目（只必填書名）", request_body=BookData, responses((status=201, description="已建立", body=Book), (status=400, description="invalid_book_* 欄位錯誤", body=ErrorBody), crate::error::CommonErrors))]
pub(crate) async fn create(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Json(data): Json<BookData>,
) -> Result<(StatusCode, Json<Book>), ApiError> {
    let data = normalize_book(data)?;
    let mut tx = state.pool.begin().await?;
    book_store::lock_catalog(&mut tx).await?;
    let book = book_store::create(&mut *tx, Some(cu.user.id), &data).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(book)))
}

async fn lock_book(conn: &mut PgConnection, id: Uuid) -> Result<Book, ApiError> {
    let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM books WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?;
    if exists.is_none() {
        return Err(ApiError::not_found());
    }
    book_store::get(&mut *conn, id)
        .await?
        .ok_or_else(ApiError::not_found)
}

#[utoipa::path(put, path="/api/books/{id}", tag="books", summary="取代書目欄位（教師只能編輯自己新增的書目）", params(("id"=Uuid, Path)), request_body=BookData, responses((status=200, description="更新後書目", body=Book), (status=400, description="invalid_book_* 欄位錯誤", body=ErrorBody), crate::error::CommonErrors))]
pub(crate) async fn update(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(data): Json<BookData>,
) -> Result<Json<Book>, ApiError> {
    let mut tx = state.pool.begin().await?;
    book_store::lock_catalog(&mut tx).await?;
    let old = lock_book(&mut tx, id).await?;
    if !cu.roles.admin && old.created_by != Some(cu.user.id) {
        return Err(ApiError::no_permission());
    }
    let data = normalize_book(data)?;
    let book = book_store::update_owned(&mut *tx, id, cu.user.id, cu.roles.admin, &data)
        .await?
        .ok_or_else(ApiError::not_found)?;
    tx.commit().await?;
    Ok(Json(book))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct BookStatus {
    is_active: bool,
}
#[utoipa::path(patch, path="/api/books/{id}/status", tag="books", summary="管理者停用或重新啟用書目", params(("id"=Uuid, Path)), request_body=BookStatus, responses((status=200, description="更新後書目", body=Book), crate::error::CommonErrors))]
pub(crate) async fn set_status(
    _: RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(data): Json<BookStatus>,
) -> Result<Json<Book>, ApiError> {
    let mut tx = state.pool.begin().await?;
    book_store::lock_catalog(&mut tx).await?;
    let book = book_store::set_active(&mut *tx, id, data.is_active)
        .await?
        .ok_or_else(ApiError::not_found)?;
    tx.commit().await?;
    Ok(Json(book))
}

#[utoipa::path(get, path="/api/books/{id}/deletion-impact", tag="books", summary="管理者確認版本刪除影響；最後一版將一併刪作品及關聯", params(("id"=Uuid, Path)), responses((status=200, description="deletes_work 表示一併刪除作品；書單數僅計實際刪除的關聯", body=BookImpact), crate::error::CommonErrors))]
pub(crate) async fn deletion_impact(
    _: RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<BookImpact>, ApiError> {
    let mut tx = state.pool.begin().await?;
    book_store::lock_catalog(&mut tx).await?;
    book_store::get(&mut *tx, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let impact = book_store::impact(&mut *tx, id).await?;
    tx.commit().await?;
    Ok(Json(impact))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct DeleteBook {
    confirm_title: String,
}
#[utoipa::path(delete, path="/api/books/{id}", tag="books", summary="管理者永久刪除版本；最後一版連同作品及書單關聯（須確認版本書名）", params(("id"=Uuid, Path)), request_body=DeleteBook, responses((status=204, description="已刪除；歷史推薦不受影響"), (status=400, description="confirmation_mismatch", body=ErrorBody), crate::error::CommonErrors))]
pub(crate) async fn remove(
    _: RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    body: Result<Json<DeleteBook>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let mut tx = state.pool.begin().await?;
    book_store::lock_catalog(&mut tx).await?;
    let book = lock_book(&mut tx, id).await?;
    ensure_confirmed(&confirmation(body)?.confirm_title, &book.title)?;
    book_store::delete(&mut *tx, id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path="/api/books/search", tag="books", summary="搜尋外部書目（不寫入書庫）", params(("q"=String, Query, description="書名、作者或 ISBN；最多 1000 字元")), responses((status=200, description="待教師選定版本的候選；正常無結果為空陣列", body=Vec<crate::book_sources::BookCandidate>), (status=400, description="invalid_filter", body=ErrorBody), (status=503, description="book_source_unavailable；保留手填內容", body=ErrorBody), crate::error::CommonErrors))]
pub(crate) async fn search(
    _: RequireTeacher,
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<crate::book_sources::BookCandidate>>, ApiError> {
    let q = query_text(&query, true)?.ok_or_else(ApiError::invalid_filter)?;
    state.book_source.search(q).await.map(Json).map_err(|_| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "book_source_unavailable",
            "外部服務暫時無法使用",
        )
    })
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct BookListInput {
    work_ids: Vec<Uuid>,
}
fn validate_ids(ids: &[Uuid]) -> Result<(), ApiError> {
    let mut unique = std::collections::HashSet::new();
    if ids.iter().any(|id| !unique.insert(id)) {
        return Err(ApiError::bad_request("invalid_book", "書目不可重複"));
    }
    Ok(())
}
async fn topic_exists(pool: &PgPool, id: Uuid) -> Result<(), ApiError> {
    if !sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM topics WHERE id=$1)")
        .bind(id)
        .fetch_one(pool)
        .await?
    {
        return Err(ApiError::not_found());
    }
    Ok(())
}

#[utoipa::path(get, path="/api/topics/{id}/books", tag="books", summary="題目預設書單", params(("id"=Uuid, Path)), responses((status=200, description="書目列表，包含已停用項目", body=Vec<BookWork>), crate::error::CommonErrors))]
pub(crate) async fn topic_list(
    _: RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<BookWork>>, ApiError> {
    topic_exists(&state.pool, id).await?;
    Ok(Json(book_store::topic_books(&state.pool, id).await?))
}

#[utoipa::path(put, path="/api/topics/{id}/books", tag="books", summary="取代題目預設書單（不影響既有活動副本）", params(("id"=Uuid, Path)), request_body=BookListInput, responses((status=200, description="更新後書單", body=Vec<BookWork>), (status=400, description="invalid_book", body=ErrorBody), crate::error::CommonErrors))]
pub(crate) async fn replace_topic_list(
    _: RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(data): Json<BookListInput>,
) -> Result<Json<Vec<BookWork>>, ApiError> {
    validate_ids(&data.work_ids)?;
    let mut tx = state.pool.begin().await?;
    book_store::lock_catalog(&mut tx).await?;
    if sqlx::query_scalar::<_, Uuid>("SELECT id FROM topics WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .is_none()
    {
        return Err(ApiError::not_found());
    }
    if !book_store::replace_topic_books(&mut tx, id, &data.work_ids).await? {
        return Err(ApiError::bad_request("invalid_book", "書目不存在或已停用"));
    }
    let rows = book_store::topic_books(&mut *tx, id).await?;
    tx.commit().await?;
    Ok(Json(rows))
}

async fn visible_activity_topic(
    pool: &PgPool,
    cu: &CurrentUser,
    id: Uuid,
    topic: Uuid,
) -> Result<(), ApiError> {
    let visible: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM activity_topics at JOIN activities a ON a.id=at.activity_id JOIN classrooms cl ON cl.id=a.classroom_id WHERE at.activity_id=$1 AND at.topic_id=$2 AND ($3 OR cl.created_by=$4))").bind(id).bind(topic).bind(cu.roles.admin).bind(cu.user.id).fetch_one(pool).await?;
    if !visible {
        return Err(ApiError::not_found());
    }
    Ok(())
}

#[utoipa::path(get, path="/api/activities/{id}/topics/{topic_id}/books", tag="books", summary="活動內單一題目的獨立書單", params(("id"=Uuid, Path), ("topic_id"=Uuid, Path)), responses((status=200, description="活動題目書單，封存時仍可讀", body=Vec<BookWork>), crate::error::CommonErrors))]
pub(crate) async fn activity_list(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path((id, topic)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<BookWork>>, ApiError> {
    visible_activity_topic(&state.pool, &cu, id, topic).await?;
    Ok(Json(
        book_store::activity_books(&state.pool, id, topic).await?,
    ))
}

#[utoipa::path(put, path="/api/activities/{id}/topics/{topic_id}/books", tag="books", summary="取代活動內單一題目書單（不回寫題目預設書單）", params(("id"=Uuid, Path), ("topic_id"=Uuid, Path)), request_body=BookListInput, responses((status=200, description="更新後書單", body=Vec<BookWork>), (status=400, description="invalid_book", body=ErrorBody), (status=409, description="activity_archived / classroom_archived", body=ErrorBody), crate::error::CommonErrors))]
pub(crate) async fn replace_activity_list(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path((id, topic)): Path<(Uuid, Uuid)>,
    Json(data): Json<BookListInput>,
) -> Result<Json<Vec<BookWork>>, ApiError> {
    validate_ids(&data.work_ids)?;
    let mut tx = state.pool.begin().await?;
    book_store::lock_catalog(&mut tx).await?;
    let row: Option<(Uuid,bool)> = sqlx::query_as("SELECT a.classroom_id,a.is_archived FROM activities a JOIN classrooms cl ON cl.id=a.classroom_id JOIN activity_topics at ON at.activity_id=a.id WHERE a.id=$1 AND at.topic_id=$2 AND ($3 OR cl.created_by=$4) FOR UPDATE OF a").bind(id).bind(topic).bind(cu.roles.admin).bind(cu.user.id).fetch_optional(&mut *tx).await?;
    let (classroom, archived) = row.ok_or_else(ApiError::not_found)?;
    crate::classrooms::ensure_writable(&mut *tx, classroom).await?;
    if archived {
        return Err(ApiError::conflict("activity_archived", "活動已封存"));
    }
    if !book_store::replace_activity_books(&mut tx, id, topic, &data.work_ids).await? {
        return Err(ApiError::bad_request("invalid_book", "書目不存在或已停用"));
    }
    let rows = book_store::activity_books(&mut *tx, id, topic).await?;
    tx.commit().await?;
    Ok(Json(rows))
}
