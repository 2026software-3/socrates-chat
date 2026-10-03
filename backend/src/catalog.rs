//! 題目（F-03）與學生可選題目（F-04）。
//!
//! 題目由教師直接建立與維護，所有教師權限相同（S-01.3）；沒有另外的「活動」。

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch},
};
use serde::{Deserialize, Deserializer, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{RequireStudent, RequireTeacher},
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
        .route("/api/topics", get(list_all_topics).post(create_topic))
        .route("/api/topics/{id}", patch(update_topic))
        .route("/api/available", get(available))
}

fn valid_text(s: &str) -> bool {
    !s.trim().is_empty()
}

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
    _: RequireTeacher,
    State(state): State<AppState>,
) -> Result<Json<Vec<Topic>>, ApiError> {
    let rows = sqlx::query_as(
        "SELECT id, title, description, category, is_active FROM topics ORDER BY created_at, id",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

async fn create_topic(
    _: RequireTeacher,
    State(state): State<AppState>,
    Json(b): Json<NewTopic>,
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
    _: RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(p): Json<TopicPatch>,
) -> Result<Json<Topic>, ApiError> {
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

// ---- 學生可選項目 ----

/// 已啟用的題目。
async fn available(
    _: RequireStudent,
    State(state): State<AppState>,
) -> Result<Json<Vec<Topic>>, ApiError> {
    let topics = sqlx::query_as(
        "SELECT id, title, description, category, is_active FROM topics
         WHERE is_active ORDER BY created_at, id",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(topics))
}
