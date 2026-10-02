//! AI 回覆串流（F-06.2、F-06.4、F-07；規格見 S-03.2、S-03.5、S-08.1、S-08.4）。
//!
//! 學生訊息先由 `POST /api/conversations/{id}/messages` 保存；前端再開啟
//! `GET /api/conversations/{id}/stream`（SSE）取得 AI 回覆。重試按鈕就是再開一次串流。
//! 即使前端中途斷線，後端仍會把完整回覆存下來。

use std::{convert::Infallible, time::Duration};

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
    time::{Instant, timeout, timeout_at},
};
use tokio_stream::wrappers::ReceiverStream;
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

type EventTx = mpsc::Sender<Event>;

fn event(name: &str, data: serde_json::Value) -> Event {
    Event::default().event(name).data(data.to_string())
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
    let messages = load_messages(&state, id).await?;
    // 只有在最後一則是學生訊息、還沒有 AI 回覆時才需要產生
    if messages.last().is_none_or(|m| m.role != "student") {
        return Err(ApiError::conflict("nothing_to_reply", "沒有待回覆的訊息"));
    }
    let (tx, rx) = mpsc::channel(64);
    tokio::spawn(generate(state, conversation, messages, tx));
    Ok(Sse::new(ReceiverStream::new(rx).map(Ok)).keep_alive(KeepAlive::default()))
}

async fn generate(state: AppState, conv: Conversation, messages: Vec<Message>, tx: EventTx) {
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

    // 自動重試 1 次，但只在尚未串流出任何內容時（S-03.5）
    let mut outcome = None;
    for _ in 0..2 {
        match attempt(&state, &req, &tx).await {
            Ok(r) => {
                outcome = Some(r);
                break;
            }
            Err(received_any) if received_any => break,
            Err(_) => {}
        }
    }
    let Some((text, meta)) = outcome else {
        let _ = tx
            .send(event("error", json!({"code": "ai_unavailable"})))
            .await;
        return;
    };

    match save_reply(&state, &conv, &text, &meta).await {
        Ok(Some(saved)) => {
            let _ = tx
                .send(event(
                    "done",
                    json!({
                        "message_id": saved.message_id,
                        "question_type": meta.question_type,
                        "stage": saved.stage,
                        "suggest_end": saved.converge_ready,
                    }),
                ))
                .await;
        }
        // 對話在串流期間已有別的回覆或已結束
        Ok(None) => {
            let _ = tx.send(event("error", json!({"code": "conflict"}))).await;
        }
        Err(_) => {
            let _ = tx.send(event("error", json!({"code": "internal"}))).await;
        }
    }
}

/// 一次 AI 呼叫。失敗時回傳「是否已經收到內容」，決定能不能自動重試。
async fn attempt(
    state: &AppState,
    req: &AiRequest,
    tx: &EventTx,
) -> Result<(String, TurnMeta), bool> {
    let first_token: Duration = state.config.ai_first_token_timeout;
    let deadline = Instant::now() + state.config.ai_total_timeout;
    let mut stream = match timeout(first_token, state.ai.stream_chat(req.clone())).await {
        Ok(Ok(s)) => s,
        _ => return Err(false),
    };
    let mut received = false;
    let mut splitter = MetaSplitter::default();
    let mut text = String::new();
    loop {
        let limit = if received {
            deadline
        } else {
            deadline.min(Instant::now() + first_token)
        };
        match timeout_at(limit, stream.next()).await {
            Err(_) | Ok(Some(Err(_))) => return Err(received),
            Ok(None) => break,
            Ok(Some(Ok(chunk))) => {
                received = true;
                let out = splitter.push(&chunk);
                if !out.is_empty() {
                    text.push_str(&out);
                    let _ = tx.send(event("delta", json!({"text": out}))).await;
                }
            }
        }
    }
    let (rest, meta_raw) = splitter.finish();
    if !rest.is_empty() {
        text.push_str(&rest);
        let _ = tx.send(event("delta", json!({"text": rest}))).await;
    }
    if text.trim().is_empty() {
        return Err(received);
    }
    Ok((text.trim().to_string(), parse_meta(&meta_raw)))
}

struct Saved {
    message_id: Uuid,
    stage: i16,
    converge_ready: bool,
}

/// 保存 AI 回覆並更新階段；最後一則已不是學生訊息時不保存（避免重複回覆）。
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
        "INSERT INTO messages (conversation_id, role, content, rules_version, question_type)
         VALUES ($1, 'ai', $2, $3, $4) RETURNING id",
    )
    .bind(conv.id)
    .bind(text)
    .bind(RULES_VERSION)
    .bind(&meta.question_type)
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
