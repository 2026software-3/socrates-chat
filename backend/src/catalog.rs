//! 題目庫（F-03）、討論活動（F-02）與學生可選項目（F-04）。
//!
//! 題目庫由教師（含助教）與管理者維護，活動由教師建立與發布，所有教師權限相同（S-01.3）。
//! 系統啟動時會補上內建題目「電車難題」（`seed_builtin_topics`）。

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{RequireAdmin, RequireStudent, RequireTeacher},
    error::ApiError,
};

/// PATCH 的三態欄位：沒帶＝`None`（保留）、`null`＝`Some(None)`（清除）、有值＝`Some(Some(v))`。
fn nullable<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/topics", get(list_all_topics).post(create_topic))
        .route("/api/admin/topics/{id}", patch(update_topic))
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
            get(get_activity).patch(update_activity),
        )
        .route("/api/activities/{id}/publish", post(publish_activity))
        .route("/api/activities/{id}/close", post(close_activity))
        .route("/api/available", get(available))
}

fn valid_text(s: &str) -> bool {
    !s.trim().is_empty()
}

// ---- 題目庫 ----

#[derive(Debug, Serialize, FromRow)]
pub struct Topic {
    pub id: Uuid,
    pub title: String,
    pub description: String,
    pub category: Option<String>,
    pub is_active: bool,
}

#[derive(Deserialize)]
struct NewTopic {
    title: String,
    #[serde(default)]
    description: String,
    category: Option<String>,
}

#[derive(Deserialize)]
struct TopicPatch {
    title: Option<String>,
    description: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    category: Option<Option<String>>,
    is_active: Option<bool>,
}

async fn list_all_topics(
    _: RequireAdmin,
    State(state): State<AppState>,
) -> Result<Json<Vec<Topic>>, ApiError> {
    list_topics(&state).await
}

async fn list_topics_for_teacher(
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

async fn create_topic(
    _: RequireAdmin,
    State(state): State<AppState>,
    Json(b): Json<NewTopic>,
) -> Result<(StatusCode, Json<Topic>), ApiError> {
    insert_topic(&state, b).await
}

async fn create_topic_for_teacher(
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

async fn update_topic(
    _: RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(p): Json<TopicPatch>,
) -> Result<Json<Topic>, ApiError> {
    patch_topic(&state, id, p).await
}

async fn update_topic_for_teacher(
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

#[derive(Debug, Serialize, FromRow)]
pub struct Activity {
    pub id: Uuid,
    pub title: String,
    pub description: String,
    pub topic_id: Option<Uuid>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

macro_rules! activity_select {
    ($tail:literal) => {
        concat!(
            "SELECT id, title, description, topic_id, status, created_at FROM activities ",
            $tail
        )
    };
}

#[derive(Deserialize)]
struct NewActivity {
    title: String,
    #[serde(default)]
    description: String,
    topic_id: Option<Uuid>,
}

#[derive(Deserialize)]
struct ActivityPatch {
    title: Option<String>,
    description: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    topic_id: Option<Option<Uuid>>,
}

async fn topic_exists(pool: &PgPool, id: Uuid) -> Result<bool, ApiError> {
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM topics WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await?;
    Ok(n > 0)
}

async fn list_activities(
    _: RequireTeacher,
    State(state): State<AppState>,
) -> Result<Json<Vec<Activity>>, ApiError> {
    let rows = sqlx::query_as(activity_select!("ORDER BY created_at DESC, id"))
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(rows))
}

async fn get_activity(
    _: RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Activity>, ApiError> {
    sqlx::query_as(activity_select!("WHERE id = $1"))
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

async fn create_activity(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Json(b): Json<NewActivity>,
) -> Result<(StatusCode, Json<Activity>), ApiError> {
    if !valid_text(&b.title) {
        return Err(ApiError::bad_request("invalid_title", "標題不可為空"));
    }
    if let Some(t) = b.topic_id
        && !topic_exists(&state.pool, t).await?
    {
        return Err(ApiError::bad_request("invalid_topic", "題目不存在"));
    }
    let a = sqlx::query_as(
        "INSERT INTO activities (title, description, topic_id, created_by) VALUES ($1, $2, $3, $4)
         RETURNING id, title, description, topic_id, status, created_at",
    )
    .bind(b.title.trim())
    .bind(b.description)
    .bind(b.topic_id)
    .bind(cu.user.id)
    .fetch_one(&state.pool)
    .await?;
    Ok((StatusCode::CREATED, Json(a)))
}

async fn update_activity(
    _: RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(p): Json<ActivityPatch>,
) -> Result<Json<Activity>, ApiError> {
    if p.title.as_deref().is_some_and(|t| !valid_text(t)) {
        return Err(ApiError::bad_request("invalid_title", "標題不可為空"));
    }
    if let Some(Some(t)) = p.topic_id
        && !topic_exists(&state.pool, t).await?
    {
        return Err(ApiError::bad_request("invalid_topic", "題目不存在"));
    }
    sqlx::query_as(
        "UPDATE activities SET title = COALESCE($2, title), description = COALESCE($3, description),
                topic_id = CASE WHEN $4 THEN $5::uuid ELSE topic_id END, updated_at = now()
         WHERE id = $1
         RETURNING id, title, description, topic_id, status, created_at",
    )
    .bind(id)
    .bind(p.title.map(|t| t.trim().to_string()))
    .bind(p.description)
    .bind(p.topic_id.is_some())
    .bind(p.topic_id.flatten())
    .fetch_optional(&state.pool)
    .await?
    .map(Json)
    .ok_or_else(ApiError::not_found)
}

async fn set_status(
    state: &AppState,
    id: Uuid,
    status: &'static str,
) -> Result<Json<Activity>, ApiError> {
    sqlx::query_as(
        "UPDATE activities SET status = $2, updated_at = now() WHERE id = $1
         RETURNING id, title, description, topic_id, status, created_at",
    )
    .bind(id)
    .bind(status)
    .fetch_optional(&state.pool)
    .await?
    .map(Json)
    .ok_or_else(ApiError::not_found)
}

async fn publish_activity(
    _: RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Activity>, ApiError> {
    set_status(&state, id, "published").await
}

async fn close_activity(
    _: RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Activity>, ApiError> {
    set_status(&state, id, "closed").await
}

// ---- 學生可選項目 ----

#[derive(Serialize)]
struct Available {
    /// 教師指定的活動（已發布）
    activities: Vec<Activity>,
    /// 題目庫中已啟用的題目
    topics: Vec<Topic>,
}

async fn available(
    _: RequireStudent,
    State(state): State<AppState>,
) -> Result<Json<Available>, ApiError> {
    let activities = sqlx::query_as(activity_select!(
        "WHERE status = 'published' ORDER BY created_at DESC, id"
    ))
    .fetch_all(&state.pool)
    .await?;
    let topics = sqlx::query_as(
        "SELECT id, title, description, category, is_active FROM topics
         WHERE is_active ORDER BY created_at, id",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(Available { activities, topics }))
}
