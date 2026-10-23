//! 對話與訊息（F-06.1、F-06.3、F-06.5；規格見 S-02.5、S-03.3、S-08.1）。
//!
//! 對話內容只有擁有者能讀寫：教師與管理者都看不到原文（S-02.1），
//! 其他人存取一律回 404，不洩漏對話是否存在。

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    AppState,
    auth::RequireStudent,
    error::ApiError,
    pagination::{self, Page, Pagination},
    reply::{self, EventStream},
    summary,
};

/// 單則訊息的長度上限（字元），避免單次請求過大。
pub const MAX_MESSAGE_CHARS: usize = 4000;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/conversations", get(list).post(create))
        .route("/api/conversations/{id}", get(detail).delete(remove))
        .route("/api/conversations/{id}/messages", post(send_message))
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
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

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub struct Message {
    pub id: Uuid,
    pub role: String,
    pub content: String,
    pub source: String,
    pub question_type: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// 對話分頁；只給 OpenAPI 描述（泛型的 `Page<T>` 會把項目欄位展開，無法 `$ref`）。
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub(crate) struct ConversationPage {
    items: Vec<Conversation>,
    next_cursor: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct NewConversation {
    activity_id: Option<Uuid>,
    topic_id: Option<Uuid>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct Detail {
    #[serde(flatten)]
    conversation: Conversation,
    messages: Vec<Message>,
}

#[utoipa::path(
    post,
    path = "/api/conversations",
    tag = "conversations",
    summary = "開始對話（活動或題目）",
    request_body = NewConversation,
    responses((status = 201, description = "對話", body = Conversation), (status = 400, description = "invalid_conversation_source", body = crate::error::ErrorBody), (status = 409, description = "activity_already_started", body = crate::error::ErrorBody))
)]
pub(crate) async fn create(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Json(b): Json<NewConversation>,
) -> Result<(StatusCode, Json<Conversation>), ApiError> {
    // 來源必須是已發布的活動，或已啟用的題目。
    // 活動列以 FOR SHARE 鎖住到交易結束：教師更換活動題目時會鎖同一列再檢查有沒有對話，
    // 所以新對話與更換題目不會同時成立（S-04.7）。
    let mut tx = state.pool.begin().await?;
    let source: Option<(Option<Uuid>, Option<Uuid>, String, String)> =
        match (b.activity_id, b.topic_id) {
            (Some(a), _) => sqlx::query_as(
                "SELECT id, topic_id, title, description FROM activities
                 WHERE id = $1 AND status = 'published' FOR SHARE",
            )
            .bind(a)
            .fetch_optional(&mut *tx)
            .await?
            .map(|(id, topic, title, desc): (Uuid, Uuid, String, String)| {
                (Some(id), Some(topic), title, desc)
            }),
            (None, Some(t)) => sqlx::query_as(
                "SELECT id, title, description FROM topics WHERE id = $1 AND is_active",
            )
            .bind(t)
            .fetch_optional(&mut *tx)
            .await?
            .map(|(id, title, desc): (Uuid, String, String)| (None, Some(id), title, desc)),
            (None, None) => {
                return Err(ApiError::bad_request(
                    "invalid_conversation_source",
                    "請選擇活動或題目",
                ));
            }
        };
    let Some((activity_id, topic_id, title, description)) = source else {
        return Err(ApiError::bad_request(
            "invalid_conversation_source",
            "活動或題目不可用",
        ));
    };
    // 每位學生在每個活動至多一場對話（S-04.7）：靠唯一索引，同時送出的兩個請求也只會成功一個
    let created = sqlx::query_as(
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
    .fetch_one(&mut *tx)
    .await;
    let c: Conversation = match created {
        Ok(c) => c,
        Err(sqlx::Error::Database(e))
            if e.constraint() == Some("conversations_user_activity_uniq") =>
        {
            return Err(ApiError::conflict(
                "activity_already_started",
                "這個活動你已經開始過了",
            ));
        }
        Err(e) => return Err(e.into()),
    };
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(c)))
}

#[utoipa::path(
    get,
    path = "/api/conversations",
    tag = "conversations",
    summary = "我的對話列表（不含訊息）",
    params(("limit" = Option<u16>, Query, description = "1–200，預設 50"), ("cursor" = Option<String>, Query, description = "前頁 next_cursor")),
    responses((status = 200, description = "對話分頁，依 created_at DESC, id ASC", body = ConversationPage), (status = 400, description = "invalid_limit / invalid_cursor", body = crate::error::ErrorBody))
)]
pub(crate) async fn list(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Query(query): Query<Pagination>,
) -> Result<Json<Page<Conversation>>, ApiError> {
    let limit = query.limit()?;
    let cursor: Option<(DateTime<Utc>, Uuid)> = query.cursor("conversations")?;
    let rows = if let Some((time, id)) = cursor {
        sqlx::query_as(conversation_select!(
            "WHERE user_id = $1 AND created_at <= $2 AND (created_at < $2 OR id > $3) ORDER BY created_at DESC, id ASC LIMIT $4"
        ))
        .bind(cu.user.id).bind(time).bind(id).bind((limit + 1) as i64)
        .fetch_all(&state.pool).await?
    } else {
        sqlx::query_as(conversation_select!(
            "WHERE user_id = $1 ORDER BY created_at DESC, id ASC LIMIT $2"
        ))
        .bind(cu.user.id)
        .bind((limit + 1) as i64)
        .fetch_all(&state.pool)
        .await?
    };
    Ok(Json(pagination::page(
        rows,
        limit,
        "conversations",
        |row: &Conversation| (row.created_at, row.id),
    )?))
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

#[utoipa::path(
    get,
    path = "/api/conversations/{id}",
    tag = "conversations",
    summary = "對話與全部訊息",
    params(("id" = Uuid, Path, description = "對話 ID")),
    responses((status = 200, description = "對話與訊息", body = Detail))
)]
pub(crate) async fn detail(
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

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct NewMessage {
    content: String,
    #[serde(default)]
    source: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/conversations/{id}/messages",
    tag = "conversations",
    summary = "送出學生訊息並以 SSE 串流 AI 回覆",
    params(("id" = Uuid, Path, description = "對話 ID")),
    request_body = NewMessage,
    responses((status = 200, description = "text/event-stream。事件依序：message（保存好的學生訊息，含 auto_ended）；若未達回合上限再接 delta {text}* 與 done {message_id, question_type, stage, suggest_end} 或 error {code}", content_type = "text/event-stream", body = String), (status = 400, description = "invalid_message / invalid_source", body = crate::error::ErrorBody), (status = 409, description = "conversation_ended / reply_in_progress", body = crate::error::ErrorBody))
)]
pub(crate) async fn send_message(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<NewMessage>,
) -> Result<EventStream, ApiError> {
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
    // AI 正在回覆時不接受新訊息，避免回覆對不上提問；同時取得這一輪產生回覆的權利
    // （達回合上限時不產生回覆，所以不 claim，讓後面的結束對話可以進行）
    let turn: Option<(i32,)> = sqlx::query_as(
        "UPDATE conversations SET turn_count = turn_count + 1,
                generating_since = CASE WHEN turn_count + 1 >= $3 THEN NULL ELSE now() END
         WHERE id = $1 AND status = 'active'
           AND (generating_since IS NULL
                OR generating_since < now() - make_interval(secs => $2))
         RETURNING turn_count",
    )
    .bind(id)
    .bind(state.config.generation_stale_secs())
    .bind(state.config.max_turns as i32)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((turn,)) = turn else {
        return Err(ApiError::conflict("reply_in_progress", "AI 正在回覆中"));
    };
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
    // 到達回合上限：不再追問，直接產生總結並結束（S-03.3）
    let auto_ended = turn >= state.config.max_turns as i32
        && summary::end_conversation(&mut tx, id, state.config.generation_stale_secs()).await?;
    tx.commit().await?;
    // 第一個事件是保存好的學生訊息；之後才是 AI 回覆的 delta／done／error
    let first = reply::event(
        "message",
        serde_json::to_value(SentMessage {
            message: m,
            auto_ended,
        })
        .map_err(|_| ApiError::internal())?,
    );
    if auto_ended {
        summary::spawn_generation(state.clone(), id, 1);
        return Ok(reply::single_event(first));
    }
    reply::begin_stream(state, cu.user.id, id, first).await
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct SentMessage {
    #[serde(flatten)]
    message: Message,
    /// 這則訊息達到回合上限，對話已自動結束、總結產生中。
    auto_ended: bool,
}

#[utoipa::path(
    delete,
    path = "/api/conversations/{id}",
    tag = "conversations",
    summary = "刪除我的對話",
    params(("id" = Uuid, Path, description = "對話 ID")),
    responses((status = 204, description = "已刪除"))
)]
pub(crate) async fn remove(
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
