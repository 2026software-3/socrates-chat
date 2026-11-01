//! 對話與訊息（F-06.1、F-06.3、F-06.5；規格見 S-02.5、S-03.3、S-08.1）。
//!
//! 對話內容只有擁有者能讀寫：教師與管理者都看不到原文（S-02.1），
//! 其他人存取一律回 404，不洩漏對話是否存在。

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::sse::Event,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    AppState, activity_store,
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
        .route("/api/conversations/{id}/archive", post(archive))
        .route("/api/conversations/{id}/unarchive", post(unarchive))
        .route("/api/conversations/{id}/messages", post(send_message))
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub struct Conversation {
    pub id: Uuid,
    pub activity_id: Option<Uuid>,
    pub topic_id: Option<Uuid>,
    /// 活動對話所屬的教室（取自活動，建立後不變）；從題目庫開始的個人練習為 null
    pub classroom_id: Option<Uuid>,
    pub title: String,
    pub description: String,
    pub language: String,
    pub status: String,
    pub stage: i16,
    pub turn_count: i32,
    pub converge_ready: bool,
    pub created_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    /// 學生已封存：只從自己的清單隱藏（S-14）
    pub is_archived: bool,
}

macro_rules! conversation_select {
    ($tail:literal) => {
        concat!(
            "SELECT id, activity_id, topic_id, classroom_id, title, description, language, status, stage,
                    turn_count, converge_ready, created_at, ended_at,
                    archived_at IS NOT NULL AS is_archived FROM conversations ",
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

/// 分頁參數加上封存篩選。
#[derive(Deserialize)]
pub(crate) struct ConversationQuery {
    #[serde(flatten)]
    page: Pagination,
    archived: Option<String>,
    /// 只列這個活動的對話
    activity_id: Option<String>,
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
    /// AI 回覆正在產生；可用 `GET /api/conversations/{id}/stream` 接上
    generating: bool,
    /// 伺服器端語音對話已啟用，可連 `GET /api/conversations/{id}/voice`
    voice_realtime: bool,
}

#[utoipa::path(
    post,
    path = "/api/conversations",
    tag = "conversations",
    summary = "開始對話（活動或題目）",
    request_body = NewConversation,
    responses((status = 201, description = "對話", body = Conversation), (status = 400, description = "invalid_conversation_source：來源不可用，含活動不存在、未發布、已封存、不在開放期間、所在教室已封存，或學生不在活動所屬教室的名單內（不區分原因）", body = crate::error::ErrorBody), (status = 409, description = "activity_already_started", body = crate::error::ErrorBody))
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
    // 活動必須在學生所屬教室的名單內；不在名單內與不存在的活動回應相同（不外露資源是否存在，S-14）。
    // 活動對話的教室取自活動；從題目庫開始的個人練習沒有教室。
    type Source = (Option<Uuid>, Option<Uuid>, Option<Uuid>, String, String);
    let source: Option<Source> =
        match (b.activity_id, b.topic_id) {
            (Some(a), _) => activity_store::find_startable(&mut tx, a, cu.user.id)
                .await?
                .map(|s| {
                    (
                        Some(s.id),
                        Some(s.topic_id),
                        Some(s.classroom_id),
                        s.title,
                        s.description,
                    )
                }),
            (None, Some(t)) => sqlx::query_as(
                "SELECT id, title, description FROM topics WHERE id = $1 AND is_active",
            )
            .bind(t)
            .fetch_optional(&mut *tx)
            .await?
            .map(|(id, title, desc): (Uuid, String, String)| (None, Some(id), None, title, desc)),
            (None, None) => {
                return Err(ApiError::bad_request(
                    "invalid_conversation_source",
                    "請選擇活動或題目",
                ));
            }
        };
    let Some((activity_id, topic_id, classroom_id, title, description)) = source else {
        return Err(ApiError::bad_request(
            "invalid_conversation_source",
            "活動或題目不可用",
        ));
    };
    // 每位學生在每個活動至多一場對話（S-04.7）：靠唯一索引，同時送出的兩個請求也只會成功一個
    let created = sqlx::query_as(
        "INSERT INTO conversations (user_id, activity_id, topic_id, classroom_id, title, description, language)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, activity_id, topic_id, classroom_id, title, description, language, status,
                   stage, turn_count, converge_ready, created_at, ended_at, archived_at IS NOT NULL AS is_archived",
    )
    .bind(cu.user.id)
    .bind(activity_id)
    .bind(topic_id)
    .bind(classroom_id)
    .bind(title)
    .bind(description)
    .bind(&cu.user.language)
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
    params(("limit" = Option<u16>, Query, description = "1–200，預設 50"), ("cursor" = Option<String>, Query, description = "前頁 next_cursor"), ("archived" = Option<bool>, Query, description = "true 只列已封存的對話；預設只列未封存的"), ("activity_id" = Option<Uuid>, Query, description = "只列這個活動的對話")),
    responses((status = 200, description = "對話分頁，依 created_at DESC, id ASC", body = ConversationPage), (status = 400, description = "invalid_limit / invalid_cursor / invalid_archived / invalid_activity_id", body = crate::error::ErrorBody))
)]
pub(crate) async fn list(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Query(query): Query<ConversationQuery>,
) -> Result<Json<Page<Conversation>>, ApiError> {
    let archived = pagination::parse_archived(query.archived.as_deref())?;
    let query_activity = query.activity_id;
    let query = query.page;
    let limit = query.limit()?;
    let cursor: Option<(DateTime<Utc>, Uuid)> = query.cursor("conversations")?;
    let (after_time, after_id) = cursor.unzip();
    let activity =
        match query_activity.as_deref() {
            Some(raw) => Some(Uuid::parse_str(raw).map_err(|_| {
                ApiError::bad_request("invalid_activity_id", "activity_id 格式不正確")
            })?),
            None => None,
        };
    let rows = sqlx::query_as(conversation_select!(
        "WHERE user_id = $1 AND (archived_at IS NOT NULL) = $2
           AND ($3::uuid IS NULL OR activity_id = $3)
           AND ($4::timestamptz IS NULL OR created_at < $4 OR (created_at = $4 AND id > $5))
         ORDER BY created_at DESC, id ASC LIMIT $6"
    ))
    .bind(cu.user.id)
    .bind(archived)
    .bind(activity)
    .bind(after_time)
    .bind(after_id)
    .bind((limit + 1) as i64)
    .fetch_all(&state.pool)
    .await?;
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
    owned_conversation_in(&state.pool, user_id, id).await
}

/// 同 [`owned_conversation`]，可在交易或指定連線中執行。
pub(crate) async fn owned_conversation_in<'e>(
    conn: impl sqlx::PgExecutor<'e>,
    user_id: Uuid,
    id: Uuid,
) -> Result<Conversation, ApiError> {
    sqlx::query_as(conversation_select!("WHERE id = $1 AND user_id = $2"))
        .bind(id)
        .bind(user_id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(ApiError::not_found)
}

/// 學生實際聽到的訊息內容與條件：被打斷的語音回覆只算聽到的部分，一個字都沒聽到就不算存在。
/// 給學生看的、給 AI 的上下文與總結都要用這一組，才會是同一份歷史。
macro_rules! heard_content {
    () => {
        "CASE WHEN heard_chars IS NULL THEN content
              ELSE rtrim(left(content, heard_chars)) END"
    };
}
macro_rules! heard_only {
    () => {
        "heard_chars IS DISTINCT FROM 0"
    };
}
pub(crate) use {heard_content, heard_only};

pub async fn load_messages(
    state: &AppState,
    conversation_id: Uuid,
) -> Result<Vec<Message>, ApiError> {
    Ok(sqlx::query_as(concat!(
        "SELECT id, role, ",
        heard_content!(),
        " AS content, source, question_type, created_at FROM messages
         WHERE conversation_id = $1 AND ",
        heard_only!(),
        " ORDER BY seq",
    ))
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
    // 先看是否正在產生再載入訊息：回覆若在兩者之間完成，訊息裡已有它，前端就不會誤判成還沒回覆
    let generating = state.live.is_live(id);
    let messages = load_messages(&state, id).await?;
    Ok(Json(Detail {
        conversation,
        messages,
        generating,
        voice_realtime: state.config.cascade_realtime_url.is_some(),
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
    responses((status = 200, description = "text/event-stream。事件依序：message（保存好的學生訊息，含 auto_ended）；若未達回合上限再接 delta {text}* 與 done {message_id, question_type, stage, suggest_end} 或 error {code}", content_type = "text/event-stream", body = String), (status = 400, description = "invalid_message / invalid_source", body = crate::error::ErrorBody), (status = 409, description = "classroom_archived / conversation_ended / reply_in_progress", body = crate::error::ErrorBody))
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
    match submit_message(&state, cu.user.id, id, content, source, false).await? {
        Submitted::Ended(first) => Ok(reply::single_event(first)),
        Submitted::Reply {
            conversation,
            first,
        } => reply::begin_stream(state, cu.user.id, conversation, first).await,
    }
}

/// `submit_message` 的結果。
pub(crate) enum Submitted {
    /// 達回合上限：對話已結束，不產生回覆。
    Ended(Event),
    /// 已保存並取得產生回覆的權利；`first` 是保存好的學生訊息事件。
    Reply {
        conversation: Conversation,
        first: Event,
    },
}

/// 保存學生訊息並更新回合數；文字與語音對話共用。`answered_only` 時，上一則學生訊息
/// 還沒有回覆就拒絕：語音對話沒有重試的畫面，不能把新的發言疊上去。
pub(crate) async fn submit_message(
    state: &AppState,
    user: Uuid,
    id: Uuid,
    content: &str,
    source: &str,
    answered_only: bool,
) -> Result<Submitted, ApiError> {
    // 封存檢查與寫入在同一個交易內，教室列的共享鎖持續到提交
    let mut tx = state.pool.begin().await?;
    let mut conversation = crate::classrooms::writable_conversation(&mut tx, user, id).await?;
    if conversation.status != "active" {
        return Err(ApiError::conflict("conversation_ended", "對話已結束"));
    }
    // 先存學生訊息，再呼叫 AI（S-03.5）；回合數與訊息在同一個交易內更新
    // AI 正在回覆時不接受新訊息，避免回覆對不上提問；同時取得這一輪產生回覆的權利
    // （達回合上限時不產生回覆，所以不 claim，讓後面的結束對話可以進行）
    let turn: Option<(i32, i16)> = sqlx::query_as(concat!(
        "UPDATE conversations SET turn_count = turn_count + 1,
                generating_since = CASE WHEN turn_count + 1 >= $3 THEN NULL ELSE now() END
         WHERE id = $1 AND status = 'active' AND ",
        generation_unclaimed!(),
        " RETURNING turn_count, stage",
    ))
    .bind(id)
    .bind(state.config.generation_stale_secs())
    .bind(state.config.max_turns as i32)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((turn, stage)) = turn else {
        return Err(ApiError::conflict("reply_in_progress", "AI 正在回覆中"));
    };
    if answered_only {
        // 上面的 UPDATE 鎖住對話列，檢查與寫入之間不會有別的訊息。沒被聽到的回覆也算回覆過
        let last: Option<(String,)> = sqlx::query_as(
            "SELECT role FROM messages WHERE conversation_id = $1 ORDER BY seq DESC LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        if last.is_some_and(|(role,)| role == "student") {
            return Err(ApiError::conflict(
                "unanswered_message",
                "上一則訊息還沒有回覆",
            ));
        }
    }
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
        return Ok(Submitted::Ended(first));
    }
    // 回合數與階段取自上面鎖住該列的 UPDATE；其餘欄位建立後不變，不必重新查詢
    conversation.turn_count = turn;
    conversation.stage = stage;
    Ok(Submitted::Reply {
        conversation,
        first,
    })
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
    // 刪除自己的對話不受教室封存限制（S-14）
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

async fn set_archived(
    state: &AppState,
    user: Uuid,
    id: Uuid,
    archived: bool,
) -> Result<StatusCode, ApiError> {
    // 封存只是自己清單的顯示狀態，不受教室封存限制，也不影響教師端與點數
    let r = sqlx::query(
        "UPDATE conversations SET archived_at = CASE WHEN $3 THEN COALESCE(archived_at, now()) END
         WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user)
    .bind(archived)
    .execute(&state.pool)
    .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/conversations/{id}/archive",
    tag = "conversations",
    summary = "封存我的對話（只從我的清單隱藏，可復原）",
    params(("id" = Uuid, Path, description = "對話 ID")),
    responses((status = 204, description = "已封存"), (status = 404, description = "not_found", body = crate::error::ErrorBody))
)]
pub(crate) async fn archive(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    set_archived(&state, cu.user.id, id, true).await
}

#[utoipa::path(
    post,
    path = "/api/conversations/{id}/unarchive",
    tag = "conversations",
    summary = "復原已封存的對話",
    params(("id" = Uuid, Path, description = "對話 ID")),
    responses((status = 204, description = "已復原"), (status = 404, description = "not_found", body = crate::error::ErrorBody))
)]
pub(crate) async fn unarchive(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    set_archived(&state, cu.user.id, id, false).await
}
