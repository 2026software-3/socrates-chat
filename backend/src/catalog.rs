//! 題目庫（F-03）、討論活動（F-02）與學生可選項目（F-04）。
//!
//! 題目庫由教師（含助教）與管理者維護，活動由教師建立與發布，所有教師權限相同（S-01.3）。
//! 系統啟動時會補上內建題目「電車難題」（`seed_builtin_topics`）。

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, patch, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{
    AppState,
    accounts::{audit, clean_reason},
    activity_store,
    auth::{RequireStudent, RequireTeacher},
    classrooms::{ClassroomFilter, filtered_classroom, visible_classroom},
    error::ApiError,
    pagination::{self, Page, Pagination},
};

pub use crate::activity_store::Activity;

/// PATCH 的三態欄位：沒帶＝`None`（保留）、`null`＝`Some(None)`（清除）、有值＝`Some(Some(v))`。
pub(crate) fn nullable<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/topics",
            get(list_topics_for_teacher).post(create_topic_for_teacher),
        )
        .route("/api/topics/{id}", patch(update_topic_for_teacher))
        .route(
            "/api/activities",
            get(list_activities).post(create_activity),
        )
        .route(
            "/api/activities/{id}",
            get(get_activity)
                .patch(update_activity)
                .delete(delete_activity),
        )
        .route("/api/activities/{id}/archive", post(archive_activity))
        .route("/api/activities/{id}/unarchive", post(unarchive_activity))
        .route("/api/activities/{id}/publish", post(publish_activity))
        .route("/api/activities/{id}/close", post(close_activity))
        .route("/api/catalog", get(available))
}

fn valid_text(s: &str) -> bool {
    !s.trim().is_empty()
}

// ---- 題目庫 ----

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub struct Topic {
    pub id: Uuid,
    pub title: String,
    pub description: String,
    pub category: Option<String>,
    pub is_active: bool,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct NewTopic {
    title: String,
    #[serde(default)]
    description: String,
    category: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct TopicPatch {
    title: Option<String>,
    description: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    category: Option<Option<String>>,
    is_active: Option<bool>,
}

#[utoipa::path(
    get,
    path = "/api/topics",
    tag = "topics",
    summary = "列出全部題目",
    responses((status = 200, description = "題目", body = Vec<Topic>))
)]
pub(crate) async fn list_topics_for_teacher(
    _: RequireTeacher,
    State(state): State<AppState>,
) -> Result<Json<Vec<Topic>>, ApiError> {
    list_topics(&state).await
}

async fn list_topics(state: &AppState) -> Result<Json<Vec<Topic>>, ApiError> {
    let rows = sqlx::query_as(
        "SELECT id, title, description, category, is_active FROM topics ORDER BY created_at, id",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[utoipa::path(
    post,
    path = "/api/topics",
    tag = "topics",
    summary = "新增題目",
    request_body = NewTopic,
    responses((status = 201, description = "已建立", body = Topic))
)]
pub(crate) async fn create_topic_for_teacher(
    _: RequireTeacher,
    State(state): State<AppState>,
    Json(b): Json<NewTopic>,
) -> Result<(StatusCode, Json<Topic>), ApiError> {
    insert_topic(&state, b).await
}

async fn insert_topic(
    state: &AppState,
    b: NewTopic,
) -> Result<(StatusCode, Json<Topic>), ApiError> {
    if !valid_text(&b.title) {
        return Err(ApiError::bad_request("invalid_title", "標題不可為空"));
    }
    let t = sqlx::query_as(
        "INSERT INTO topics (title, description, category) VALUES ($1, $2, $3)
         RETURNING id, title, description, category, is_active",
    )
    .bind(b.title.trim())
    .bind(b.description)
    .bind(b.category)
    .fetch_one(&state.pool)
    .await?;
    Ok((StatusCode::CREATED, Json(t)))
}

#[utoipa::path(
    patch,
    path = "/api/topics/{id}",
    tag = "topics",
    summary = "修改題目（merge-patch）",
    params(("id" = Uuid, Path, description = "題目 ID")),
    request_body = TopicPatch,
    responses((status = 200, description = "更新後的題目", body = Topic))
)]
pub(crate) async fn update_topic_for_teacher(
    _: RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(p): Json<TopicPatch>,
) -> Result<Json<Topic>, ApiError> {
    patch_topic(&state, id, p).await
}

async fn patch_topic(state: &AppState, id: Uuid, p: TopicPatch) -> Result<Json<Topic>, ApiError> {
    if p.title.as_deref().is_some_and(|t| !valid_text(t)) {
        return Err(ApiError::bad_request("invalid_title", "標題不可為空"));
    }
    sqlx::query_as(
        "UPDATE topics SET title = COALESCE($2, title), description = COALESCE($3, description),
                category = CASE WHEN $4 THEN $5::text ELSE category END,
                is_active = COALESCE($6, is_active),
                updated_at = now()
         WHERE id = $1
         RETURNING id, title, description, category, is_active",
    )
    .bind(id)
    .bind(p.title.map(|t| t.trim().to_string()))
    .bind(p.description)
    .bind(p.category.is_some())
    .bind(p.category.flatten())
    .bind(p.is_active)
    .fetch_optional(&state.pool)
    .await?
    .map(Json)
    .ok_or_else(ApiError::not_found)
}

/// 內建題目的固定 ID：重複啟動時不會重複建立，教師改動或停用後也不會被覆寫。
const TROLLEY_TOPIC_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_0001);

/// 補上內建題目「電車難題」；已存在（含被教師修改或停用）時不動它。
pub async fn seed_builtin_topics(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO topics (id, title, description, category) VALUES ($1, $2, $3, $4)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(TROLLEY_TOPIC_ID)
    .bind("電車難題")
    .bind(
        "一輛失控的電車正衝向軌道上的五個人。你站在轉轍器旁，只要拉下拉桿，電車就會轉向另一條軌道，\
         但那條軌道上有一個人。你會拉下拉桿嗎？請說明你的理由。",
    )
    .bind("倫理學")
    .execute(pool)
    .await?;
    Ok(())
}

// ---- 活動 ----

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct NewActivity {
    title: String,
    #[serde(default)]
    description: String,
    /// 必填；沒帶或為 `null` 回 `invalid_topic`，讓錯誤格式與其他題目錯誤一致
    topic_id: Option<Uuid>,
    /// 活動所屬的教室（必填、建立後不可更改）；沒帶回 `invalid_classroom`，
    /// 教室不存在或不是自己管理的教室一律回 `404 not_found`
    classroom_id: Option<Uuid>,
    opens_at: Option<DateTime<Utc>>,
    closes_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct ActivityPatch {
    title: Option<String>,
    description: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    topic_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "nullable")]
    opens_at: Option<Option<DateTime<Utc>>>,
    #[serde(default, deserialize_with = "nullable")]
    closes_at: Option<Option<DateTime<Utc>>>,
}

fn validate_schedule(
    opens: Option<DateTime<Utc>>,
    closes: Option<DateTime<Utc>>,
) -> Result<(), ApiError> {
    if matches!((opens, closes), (Some(o), Some(c)) if c <= o) {
        return Err(ApiError::bad_request(
            "invalid_schedule",
            "結束時間必須晚於開始時間",
        ));
    }
    Ok(())
}

async fn topic_exists(pool: &PgPool, id: Uuid) -> Result<bool, ApiError> {
    let (exists,): (bool,) = sqlx::query_as("SELECT EXISTS (SELECT 1 FROM topics WHERE id = $1)")
        .bind(id)
        .fetch_one(pool)
        .await?;
    Ok(exists)
}

/// 活動分頁；只給 OpenAPI 描述（泛型的 `Page<T>` 會把項目欄位展開，無法 `$ref`）。
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub(crate) struct ActivityPage {
    items: Vec<Activity>,
    next_cursor: Option<String>,
}

/// 分頁參數加上教室篩選。
#[derive(Deserialize)]
pub(crate) struct ActivityQuery {
    #[serde(flatten)]
    page: Pagination,
    #[serde(flatten)]
    classroom: ClassroomFilter,
    /// true 只列已封存的活動；預設只列未封存的
    archived: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/activities",
    tag = "activities",
    summary = "列出活動（自己管理的教室內的活動；管理者為全部；草稿、已發布、已停止開放都列出）",
    params(
        ("limit" = Option<u16>, Query, description = "1–200，預設 50"),
        ("cursor" = Option<String>, Query, description = "前頁 next_cursor"),
        ("classroom_id" = Option<Uuid>, Query, description = "只看這間教室；不是自己管理的教室回 404"),
        ("archived" = Option<bool>, Query, description = "true 只列已封存的活動；預設只列未封存的"),
    ),
    responses((status = 200, description = "活動分頁，依 created_at DESC, id ASC", body = ActivityPage), (status = 400, description = "invalid_limit / invalid_cursor", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn list_activities(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Query(query): Query<ActivityQuery>,
) -> Result<Json<Page<Activity>>, ApiError> {
    let limit = query.page.limit()?;
    let after: Option<(DateTime<Utc>, Uuid)> = query.page.cursor("activities")?;
    let classroom = filtered_classroom(&state, &cu, &query.classroom).await?;
    let rows = activity_store::list(
        &state.pool,
        cu.user.id,
        cu.roles.admin,
        classroom,
        query.archived.as_deref() == Some("true"),
        after,
        (limit + 1) as i64,
    )
    .await?;
    Ok(Json(pagination::page(rows, limit, "activities", |row| {
        (row.created_at, row.id)
    })?))
}

#[utoipa::path(
    get,
    path = "/api/activities/{id}",
    tag = "activities",
    summary = "讀取活動",
    params(("id" = Uuid, Path, description = "活動 ID")),
    responses((status = 200, description = "活動", body = Activity), crate::error::CommonErrors)
)]
pub(crate) async fn get_activity(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Activity>, ApiError> {
    activity_store::get_visible(&state.pool, id, cu.user.id, cu.roles.admin)
        .await?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

#[utoipa::path(
    post,
    path = "/api/activities",
    tag = "activities",
    summary = "在教室內建立活動（草稿）",
    request_body = NewActivity,
    responses((status = 201, description = "已建立", body = Activity), (status = 400, description = "invalid_schedule / invalid_title / invalid_topic / invalid_classroom", body = crate::error::ErrorBody), (status = 404, description = "not_found：教室不存在或不是自己管理的教室", body = crate::error::ErrorBody), (status = 409, description = "classroom_archived", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn create_activity(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Json(b): Json<NewActivity>,
) -> Result<(StatusCode, Json<Activity>), ApiError> {
    if !valid_text(&b.title) {
        return Err(ApiError::bad_request("invalid_title", "標題不可為空"));
    }
    let Some(topic_id) = b.topic_id else {
        return Err(ApiError::bad_request("invalid_topic", "請選擇題目"));
    };
    let Some(classroom_id) = b.classroom_id else {
        return Err(ApiError::bad_request("invalid_classroom", "請選擇教室"));
    };
    // 不是自己管理的教室與不存在的教室回應相同
    let classroom = visible_classroom(&state, &cu, &classroom_id.to_string()).await?;
    if !topic_exists(&state.pool, topic_id).await? {
        return Err(ApiError::bad_request("invalid_topic", "題目不存在"));
    }
    validate_schedule(b.opens_at, b.closes_at)?;
    let mut tx = state.pool.begin().await?;
    crate::classrooms::ensure_writable(&mut *tx, classroom.id).await?;
    let a = activity_store::create_with_schedule(
        &mut *tx,
        classroom.id,
        cu.user.id,
        b.title.trim(),
        &b.description,
        topic_id,
        (b.opens_at, b.closes_at),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(a)))
}

#[utoipa::path(
    patch,
    path = "/api/activities/{id}",
    tag = "activities",
    summary = "修改活動（merge-patch）",
    params(("id" = Uuid, Path, description = "活動 ID")),
    request_body = ActivityPatch,
    responses((status = 200, description = "更新後的活動", body = Activity), (status = 400, description = "invalid_schedule / invalid_title / invalid_topic", body = crate::error::ErrorBody), (status = 409, description = "activity_topic_locked / classroom_archived", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn update_activity(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(p): Json<ActivityPatch>,
) -> Result<Json<Activity>, ApiError> {
    if p.title.as_deref().is_some_and(|t| !valid_text(t)) {
        return Err(ApiError::bad_request("invalid_title", "標題不可為空"));
    }
    // 活動必須有題目：不能清除，只能換成另一個存在的題目
    let new_topic = match p.topic_id {
        Some(None) => return Err(ApiError::bad_request("invalid_topic", "請選擇題目")),
        Some(Some(t)) => {
            if !topic_exists(&state.pool, t).await? {
                return Err(ApiError::bad_request("invalid_topic", "題目不存在"));
            }
            Some(t)
        }
        None => None,
    };
    // 鎖住活動列再檢查對話：開始對話時會以 FOR SHARE 鎖同一列，
    // 所以「檢查沒有對話」與「更換題目」之間不會有新對話插進來。
    let mut tx = state.pool.begin().await?;
    type CurrentSchedule = (Uuid, Uuid, Option<DateTime<Utc>>, Option<DateTime<Utc>>);
    let current: Option<CurrentSchedule> = sqlx::query_as(
        "SELECT a.topic_id, a.classroom_id, a.opens_at, a.closes_at FROM activities a JOIN classrooms cl ON cl.id = a.classroom_id
         WHERE a.id = $1 AND ($2 OR cl.created_by = $3) FOR UPDATE OF a",
    )
    .bind(id)
    .bind(cu.roles.admin)
    .bind(cu.user.id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((current_topic, classroom, old_opens, old_closes)) = current else {
        return Err(ApiError::not_found());
    };
    crate::classrooms::ensure_writable(&mut *tx, classroom).await?;
    let opens_at = p.opens_at.unwrap_or(old_opens);
    let closes_at = p.closes_at.unwrap_or(old_closes);
    validate_schedule(opens_at, closes_at)?;
    if let Some(t) = new_topic
        && t != current_topic
    {
        let (started,): (bool,) =
            sqlx::query_as("SELECT EXISTS (SELECT 1 FROM conversations WHERE activity_id = $1)")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
        if started {
            return Err(ApiError::conflict(
                "activity_topic_locked",
                "已有學生開始對話，不能更換題目",
            ));
        }
    }
    let activity = sqlx::query_as(
        "UPDATE activities SET title = COALESCE($2, title), description = COALESCE($3, description),
                topic_id = COALESCE($4, topic_id), opens_at = $5, closes_at = $6, updated_at = now()
         WHERE id = $1
         RETURNING id, title, description, topic_id, classroom_id, status, is_archived, opens_at, closes_at, created_at",
    )
    .bind(id)
    .bind(p.title.map(|t| t.trim().to_string()))
    .bind(p.description)
    .bind(new_topic)
    .bind(opens_at)
    .bind(closes_at)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(activity))
}

async fn set_status(
    state: &AppState,
    cu: &crate::auth::CurrentUser,
    id: Uuid,
    status: &'static str,
) -> Result<Json<Activity>, ApiError> {
    let activity = activity_store::get_visible(&state.pool, id, cu.user.id, cu.roles.admin)
        .await?
        .ok_or_else(ApiError::not_found)?;
    crate::classrooms::ensure_writable(&state.pool, activity.classroom_id).await?;
    activity_store::set_status_visible(&state.pool, id, cu.user.id, cu.roles.admin, status)
        .await?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

#[utoipa::path(
    post,
    path = "/api/activities/{id}/publish",
    tag = "activities",
    summary = "發布（或重新開放）活動",
    params(("id" = Uuid, Path, description = "活動 ID")),
    responses((status = 200, description = "活動", body = Activity), (status = 409, description = "classroom_archived", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn publish_activity(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Activity>, ApiError> {
    set_status(&state, &cu, id, "published").await
}

#[utoipa::path(
    post,
    path = "/api/activities/{id}/close",
    tag = "activities",
    summary = "停止開放活動",
    params(("id" = Uuid, Path, description = "活動 ID")),
    responses((status = 200, description = "活動", body = Activity), (status = 409, description = "classroom_archived", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn close_activity(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Activity>, ApiError> {
    set_status(&state, &cu, id, "closed").await
}

#[derive(Deserialize)]
pub(crate) struct ReasonQuery {
    reason: Option<String>,
}

async fn set_archived(
    state: &AppState,
    cu: &crate::auth::CurrentUser,
    id: Uuid,
    archived: bool,
    reason: Option<String>,
) -> Result<Json<Activity>, ApiError> {
    let activity = activity_store::get_visible(&state.pool, id, cu.user.id, cu.roles.admin)
        .await?
        .ok_or_else(ApiError::not_found)?;
    crate::classrooms::ensure_writable(&state.pool, activity.classroom_id).await?;
    let updated =
        activity_store::set_archived_visible(&state.pool, id, cu.user.id, cu.roles.admin, archived)
            .await?
            .ok_or_else(ApiError::not_found)?;
    let action = if archived {
        "activity_archived"
    } else {
        "activity_restored"
    };
    audit(
        &state.pool,
        cu.user.id,
        action,
        &updated.title,
        clean_reason(reason).as_deref(),
    )
    .await?;
    Ok(Json(updated))
}

#[utoipa::path(
    post,
    path = "/api/activities/{id}/archive",
    tag = "activities",
    summary = "封存活動（學生看不到、不能開新對話，既有對話照舊；可復原）",
    params(("id" = Uuid, Path, description = "活動 ID"), ("reason" = Option<String>, Query, description = "選填原因，寫入稽核日誌")),
    responses((status = 200, description = "活動", body = Activity), (status = 409, description = "classroom_archived", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn archive_activity(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<ReasonQuery>,
) -> Result<Json<Activity>, ApiError> {
    set_archived(&state, &cu, id, true, q.reason).await
}

#[utoipa::path(
    post,
    path = "/api/activities/{id}/unarchive",
    tag = "activities",
    summary = "復原已封存的活動",
    params(("id" = Uuid, Path, description = "活動 ID"), ("reason" = Option<String>, Query, description = "選填原因，寫入稽核日誌")),
    responses((status = 200, description = "活動", body = Activity), (status = 409, description = "classroom_archived", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn unarchive_activity(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<ReasonQuery>,
) -> Result<Json<Activity>, ApiError> {
    set_archived(&state, &cu, id, false, q.reason).await
}

/// 刪除活動的確認：必須與活動標題相同（S-14）。
#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct DeleteActivityBody {
    confirm_title: String,
    /// 選填原因，寫入稽核日誌
    #[schema(max_length = 500)]
    reason: Option<String>,
}

#[utoipa::path(
    delete,
    path = "/api/activities/{id}",
    tag = "activities",
    summary = "永久刪除活動，連同該活動的所有對話與總結（不可復原）",
    params(("id" = Uuid, Path, description = "活動 ID")),
    request_body = DeleteActivityBody,
    responses((status = 204, description = "已刪除"), (status = 400, description = "confirmation_mismatch", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn delete_activity(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<DeleteActivityBody>,
) -> Result<StatusCode, ApiError> {
    let activity = activity_store::get_visible(&state.pool, id, cu.user.id, cu.roles.admin)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if body.confirm_title.trim() != activity.title.trim() {
        return Err(ApiError::bad_request(
            "confirmation_mismatch",
            "確認的名稱與要刪除的活動不符",
        ));
    }
    // 對話、訊息、總結與主張分組由外鍵連動刪除；封存的教室也可以刪除其中的活動
    let mut tx = state.pool.begin().await?;
    let deleted = sqlx::query("DELETE FROM activities WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if deleted == 0 {
        return Err(ApiError::not_found());
    }
    audit(
        &mut *tx,
        cu.user.id,
        "activity_deleted",
        &activity.title,
        clean_reason(body.reason).as_deref(),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- 學生可選項目 ----

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct Available {
    /// 教師指定的活動：已發布，且在學生所屬教室的名單內
    activities: Vec<Activity>,
    /// 題目庫中已啟用的題目
    topics: Vec<Topic>,
}

#[utoipa::path(
    get,
    path = "/api/catalog",
    tag = "catalog",
    summary = "學生可選的活動與題目（活動須已發布且在開放期間內；教室未封存）",
    responses((status = 200, description = "可選項目", body = Available))
)]
pub(crate) async fn available(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
) -> Result<Json<Available>, ApiError> {
    let (activities, topics) = tokio::try_join!(
        activity_store::list_published_for_student(&state.pool, &cu.user.email),
        sqlx::query_as(
            "SELECT id, title, description, category, is_active FROM topics
             WHERE is_active ORDER BY created_at, id",
        )
        .fetch_all(&state.pool),
    )?;
    Ok(Json(Available { activities, topics }))
}
