//! AI 回覆串流（F-06.2、F-06.4、F-07；規格見 S-03.2、S-03.5、S-08.1、S-08.4）。
//!
//! 學生訊息先由 `POST /api/conversations/{id}/messages` 保存；前端再開啟
//! `GET /api/conversations/{id}/stream`（SSE）取得 AI 回覆。重試按鈕就是再開一次串流。
//! 即使前端中途斷線，後端仍會把完整回覆存下來。
//!
//! 同一場對話同時間只允許一條回覆串流：開始前在資料庫原子 claim（`generating_since`），
//! 結束時釋放；服務異常中斷造成的殘留 claim 會在逾時上限後視為過期。

use std::convert::Infallible;

use axum::{
    Router,
    extract::{Path, State},
    response::sse::{Event, KeepAlive, Sse},
    routing::get,
};
use futures_util::{Stream, StreamExt};
use serde_json::json;
use tokio::{
    sync::mpsc,
    time::{Instant, timeout_at},
};
use tokio_stream::wrappers::UnboundedReceiverStream;
use uuid::Uuid;

use crate::{
    AppState,
    ai::{
        AiRequest, ChatRole, ChatTurn, MetaSplitter, PromptContext, RULES_VERSION, TurnMeta,
        build_system_prompt, parse_meta,
    },
    auth::RequireStudent,
    chat::{Conversation, Message, load_messages, owned_conversation},
    error::ApiError,
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/conversations/{id}/stream", get(stream))
}

/// 事件通道不設上限：一則回覆很小，且這樣慢速客戶端不會拖住 AI 的逾時計時。
type EventTx = mpsc::UnboundedSender<Event>;

fn event(name: &str, data: serde_json::Value) -> Event {
    Event::default().event(name).data(data.to_string())
}

/// 嘗試取得這場對話的「正在產生回覆」權利；已有進行中的回覆時回傳 `false`。
pub async fn claim_generation(state: &AppState, id: Uuid) -> Result<bool, sqlx::Error> {
    let r = sqlx::query(
        "UPDATE conversations SET generating_since = now()
         WHERE id = $1 AND status = 'active'
           AND (generating_since IS NULL
                OR generating_since < now() - make_interval(secs => $2))",
    )
    .bind(id)
    .bind(state.config.generation_stale_secs())
    .execute(&state.pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

async fn release_generation(state: &AppState, id: Uuid) {
    if let Err(e) = sqlx::query("UPDATE conversations SET generating_since = NULL WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await
    {
        tracing::error!(error = %e, "failed to release generation claim");
    }
}

async fn stream(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let conversation = owned_conversation(&state, cu.user.id, id).await?;
    if conversation.status != "active" {
        return Err(ApiError::conflict("conversation_ended", "對話已結束"));
    }
    if !claim_generation(&state, id).await? {
        return Err(ApiError::conflict("reply_in_progress", "AI 正在回覆中"));
    }
    // claim 之後再檢查，確保檢查與產生之間不會有另一條串流插入
    let messages = match load_messages(&state, id).await {
        Ok(m) => m,
        Err(e) => {
            release_generation(&state, id).await;
            return Err(e);
        }
    };
    // 只有在最後一則是學生訊息、還沒有 AI 回覆時才需要產生
    if messages.last().is_none_or(|m| m.role != "student") {
        release_generation(&state, id).await;
        return Err(ApiError::conflict("nothing_to_reply", "沒有待回覆的訊息"));
    }
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        generate(&state, &conversation, &messages, &tx).await;
        release_generation(&state, conversation.id).await;
    });
    Ok(Sse::new(UnboundedReceiverStream::new(rx).map(Ok)).keep_alive(KeepAlive::default()))
}

async fn generate(state: &AppState, conv: &Conversation, messages: &[Message], tx: &EventTx) {
    let req = AiRequest {
        system: build_system_prompt(&PromptContext {
            title: &conv.title,
            description: &conv.description,
            stage: conv.stage,
            turn: conv.turn_count,
            wrap_up_turn: state.config.wrap_up_turn,
        }),
        messages: messages
            .iter()
            .map(|m| ChatTurn {
                role: if m.role == "student" {
                    ChatRole::User
                } else {
                    ChatRole::Assistant
                },
                content: m.content.clone(),
            })
            .collect(),
    };

    // 自動重試 1 次，但只在尚未串流出任何內容給學生時（S-03.5）
    let mut outcome = None;
    for _ in 0..2 {
        match attempt(state, &req, tx).await {
            Ok(r) => {
                outcome = Some(r);
                break;
            }
            Err(emitted_any) if emitted_any => break,
            Err(_) => {}
        }
    }
    let Some((text, meta)) = outcome else {
        let _ = tx.send(event("error", json!({"code": "ai_unavailable"})));
        return;
    };

    match save_reply(state, conv, &text, &meta).await {
        Ok(Some(saved)) => {
            let _ = tx.send(event(
                "done",
                json!({
                    "message_id": saved.message_id,
                    "question_type": meta.question_type,
                    "stage": saved.stage,
                    "suggest_end": saved.converge_ready,
                }),
            ));
        }
        // 對話在串流期間已結束或已有回覆
        Ok(None) => {
            let _ = tx.send(event("error", json!({"code": "conflict"})));
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to save AI reply");
            let _ = tx.send(event("error", json!({"code": "internal"})));
        }
    }
}

/// 一次 AI 呼叫。失敗時回傳「是否已經送出內容給學生」，決定能不能自動重試。
async fn attempt(
    state: &AppState,
    req: &AiRequest,
    tx: &EventTx,
) -> Result<(String, TurnMeta), bool> {
    let start = Instant::now();
    // 建立連線與等待第一個片段共用同一個首 token 期限（S-03.5）
    let first_token_deadline = start + state.config.ai_first_token_timeout;
    let total_deadline = start + state.config.ai_total_timeout;

    let mut stream = match timeout_at(first_token_deadline, state.ai.stream_chat(req.clone())).await
    {
        Ok(Ok(s)) => s,
        _ => return Err(false),
    };
    let mut got_chunk = false;
    let mut emitted = false;
    let mut splitter = MetaSplitter::default();
    let mut text = String::new();
    loop {
        let limit = if got_chunk {
            total_deadline
        } else {
            first_token_deadline.min(total_deadline)
        };
        match timeout_at(limit, stream.next()).await {
            Err(_) | Ok(Some(Err(_))) => return Err(emitted),
            Ok(None) => break,
            Ok(Some(Ok(chunk))) => {
                got_chunk = true;
                let out = splitter.push(&chunk);
                if !out.is_empty() {
                    emitted = true;
                    text.push_str(&out);
                    let _ = tx.send(event("delta", json!({"text": out})));
                }
            }
        }
    }
    let (rest, meta_raw) = splitter.finish();
    if !rest.is_empty() {
        emitted = true;
        text.push_str(&rest);
        let _ = tx.send(event("delta", json!({"text": rest})));
    }
    if text.trim().is_empty() {
        return Err(emitted);
    }
    Ok((text.trim().to_string(), parse_meta(&meta_raw)))
}

struct Saved {
    message_id: Uuid,
    stage: i16,
    converge_ready: bool,
}

/// 保存 AI 回覆並更新階段；對話已結束或最後一則已不是學生訊息時不保存。
async fn save_reply(
    state: &AppState,
    conv: &Conversation,
    text: &str,
    meta: &TurnMeta,
) -> Result<Option<Saved>, sqlx::Error> {
    let mut tx = state.pool.begin().await?;
    let (status, stage, ready): (String, i16, bool) = sqlx::query_as(
        "SELECT status, stage, converge_ready FROM conversations WHERE id = $1 FOR UPDATE",
    )
    .bind(conv.id)
    .fetch_one(&mut *tx)
    .await?;
    let last: Option<(String,)> = sqlx::query_as(
        "SELECT role FROM messages WHERE conversation_id = $1 ORDER BY seq DESC LIMIT 1",
    )
    .bind(conv.id)
    .fetch_optional(&mut *tx)
    .await?;
    if status != "active" || last.is_none_or(|(r,)| r != "student") {
        return Ok(None);
    }
    let (message_id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO messages (conversation_id, role, content, rules_version, question_type,
                               advance_reason)
         VALUES ($1, 'ai', $2, $3, $4, $5) RETURNING id",
    )
    .bind(conv.id)
    .bind(text)
    .bind(RULES_VERSION)
    .bind(&meta.question_type)
    .bind(&meta.reason)
    .fetch_one(&mut *tx)
    .await?;
    // 階段只進不退；第 3 階段達成條件即符合收斂條件（S-03.2、S-03.3）
    let new_stage = if meta.advance && stage < 3 {
        stage + 1
    } else {
        stage
    };
    let converge_ready = ready || (stage == 3 && meta.advance);
    sqlx::query("UPDATE conversations SET stage = $2, converge_ready = $3 WHERE id = $1")
        .bind(conv.id)
        .bind(new_stage)
        .bind(converge_ready)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Some(Saved {
        message_id,
        stage: new_stage,
        converge_ready,
    }))
}
