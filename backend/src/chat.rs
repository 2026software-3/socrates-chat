//! 對話與訊息（F-06.1、F-06.3、F-06.5；規格見 S-02.5、S-03.3、S-08.1）。
//!
//! 對話內容只有擁有者能讀寫：教師與管理者都看不到原文（S-02.1），
//! 其他人存取一律回 404，不洩漏對話是否存在。

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{AppState, auth::RequireStudent, error::ApiError};

/// 單則訊息的長度上限（字元），避免單次請求過大。
pub const MAX_MESSAGE_CHARS: usize = 4000;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/conversations", get(list).post(create))
        .route("/api/conversations/{id}", get(detail).delete(remove))
        .route("/api/conversations/{id}/messages", post(send_message))
}

#[derive(Debug, Serialize, FromRow)]
pub struct Conversation {
    pub id: Uuid,
    pub activity_id: Option<Uuid>,
    pub topic_id: Option<Uuid>,
    pub title: String,
    pub description: String,
    pub language: String,
    pub status: String,
    pub stage: i16,
    pub turn_count: i32,
    pub converge_ready: bool,
    pub created_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

macro_rules! conversation_select {
    ($tail:literal) => {
        concat!(
            "SELECT id, activity_id, topic_id, title, description, language, status, stage,
                    turn_count, converge_ready, created_at, ended_at FROM conversations ",
            $tail
        )
    };
}

#[derive(Debug, Serialize, FromRow)]
pub struct Message {
    pub id: Uuid,
    pub role: String,
    pub content: String,
    pub source: String,
    pub question_type: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct NewConversation {
    activity_id: Option<Uuid>,
    topic_id: Option<Uuid>,
}

#[derive(Serialize)]
struct Detail {
    #[serde(flatten)]
    conversation: Conversation,
    messages: Vec<Message>,
}

async fn create(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Json(b): Json<NewConversation>,
) -> Result<(StatusCode, Json<Conversation>), ApiError> {
    // 來源必須是已發布的活動，或已啟用的題目
    let source: Option<(Option<Uuid>, Option<Uuid>, String, String)> =
        match (b.activity_id, b.topic_id) {
            (Some(a), _) => sqlx::query_as(
                "SELECT id, topic_id, title, description FROM activities
                 WHERE id = $1 AND status = 'published'",
            )
            .bind(a)
            .fetch_optional(&state.pool)
            .await?
            .map(
                |(id, topic, title, desc): (Uuid, Option<Uuid>, String, String)| {
                    (Some(id), topic, title, desc)
                },
            ),
            (None, Some(t)) => sqlx::query_as(
                "SELECT id, title, description FROM topics WHERE id = $1 AND is_active",
            )
            .bind(t)
            .fetch_optional(&state.pool)
            .await?
            .map(|(id, title, desc): (Uuid, String, String)| (None, Some(id), title, desc)),
            (None, None) => {
                return Err(ApiError::bad_request("invalid_source", "請選擇活動或題目"));
            }
        };
    let Some((activity_id, topic_id, title, description)) = source else {
        return Err(ApiError::bad_request("invalid_source", "活動或題目不可用"));
    };
    let c = sqlx::query_as(
        "INSERT INTO conversations (user_id, activity_id, topic_id, title, description)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, activity_id, topic_id, title, description, language, status, stage,
                   turn_count, converge_ready, created_at, ended_at",
    )
    .bind(cu.user.id)
    .bind(activity_id)
    .bind(topic_id)
    .bind(title)
    .bind(description)
    .fetch_one(&state.pool)
    .await?;
    Ok((StatusCode::CREATED, Json(c)))
}

async fn list(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
) -> Result<Json<Vec<Conversation>>, ApiError> {
    let rows = sqlx::query_as(conversation_select!(
        "WHERE user_id = $1 ORDER BY created_at DESC, id"
    ))
    .bind(cu.user.id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

/// 取得「自己的」對話；別人的或不存在的一律 404。
pub async fn owned_conversation(
    state: &AppState,
    user_id: Uuid,
    id: Uuid,
) -> Result<Conversation, ApiError> {
    sqlx::query_as(conversation_select!("WHERE id = $1 AND user_id = $2"))
        .bind(id)
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(ApiError::not_found)
}

pub async fn load_messages(
    state: &AppState,
    conversation_id: Uuid,
) -> Result<Vec<Message>, ApiError> {
    Ok(sqlx::query_as(
        "SELECT id, role, content, source, question_type, created_at FROM messages
         WHERE conversation_id = $1 ORDER BY seq",
    )
    .bind(conversation_id)
    .fetch_all(&state.pool)
    .await?)
}

async fn detail(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Detail>, ApiError> {
    let conversation = owned_conversation(&state, cu.user.id, id).await?;
    let messages = load_messages(&state, id).await?;
    Ok(Json(Detail {
        conversation,
        messages,
    }))
}

#[derive(Deserialize)]
struct NewMessage {
    content: String,
    #[serde(default)]
    source: Option<String>,
}

async fn send_message(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<NewMessage>,
) -> Result<(StatusCode, Json<Message>), ApiError> {
    let content = b.content.trim();
    if content.is_empty() || content.chars().count() > MAX_MESSAGE_CHARS {
        return Err(ApiError::bad_request("invalid_message", "訊息長度不符"));
    }
    let source = match b.source.as_deref() {
        None | Some("text") => "text",
        Some("speech-browser") => "speech-browser",
        Some("speech-openai") => "speech-openai",
        Some(_) => return Err(ApiError::bad_request("invalid_source", "訊息來源不正確")),
    };
    let conversation = owned_conversation(&state, cu.user.id, id).await?;
    if conversation.status != "active" {
        return Err(ApiError::conflict("conversation_ended", "對話已結束"));
    }
    // 先存學生訊息，再呼叫 AI（S-03.5）；回合數與訊息在同一個交易內更新
    let mut tx = state.pool.begin().await?;
    sqlx::query("UPDATE conversations SET turn_count = turn_count + 1 WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let m = sqlx::query_as(
        "INSERT INTO messages (conversation_id, role, content, source)
         VALUES ($1, 'student', $2, $3)
         RETURNING id, role, content, source, question_type, created_at",
    )
    .bind(id)
    .bind(content)
    .bind(source)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(m)))
}

async fn remove(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    // 該場的訊息（以及之後的總結與分析）一併刪除
    let r = sqlx::query("DELETE FROM conversations WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(cu.user.id)
        .execute(&state.pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}
