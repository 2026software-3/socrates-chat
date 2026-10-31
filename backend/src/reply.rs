//! AI 回覆串流（F-06.2、F-06.4、F-07；規格見 S-03.2、S-03.5、S-08.1、S-08.4）。
//!
//! `POST /api/conversations/{id}/messages` 保存學生訊息後，直接以 SSE 串流 AI 回覆
//! （`chat::send_message` 呼叫 `begin_stream`）。`GET /api/conversations/{id}/stream` 用於：
//! - 重試：最後一則是學生訊息、還沒有 AI 回覆時，再開一次串流；
//! - 接回：回覆還在產生（例如學生離開對話頁又回來），接上同一個回覆，先補上已產生的文字。
//!
//! 即使前端中途斷線，後端仍會把完整回覆存下來。
//!
//! 同一場對話同時間只允許一條回覆串流：開始前在資料庫原子 claim（`generating_since`），
//! 結束時釋放；服務異常中斷造成的殘留 claim 會在逾時上限後視為過期。

use std::{
    collections::HashMap,
    convert::Infallible,
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    extract::{Path, State},
    response::sse::{Event, KeepAlive, KeepAliveStream, Sse},
    routing::get,
};
use futures_util::{FutureExt, StreamExt, stream::BoxStream};
use serde::Serialize;
use serde_json::json;
use tokio::{
    sync::mpsc,
    time::{Instant, timeout_at},
};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::Instrument;
use uuid::Uuid;

use crate::{
    AppState,
    ai::{
        AiRequest, ChatRole, ChatTurn, MetaSplitter, PromptContext, TurnMeta, build_system_prompt,
        parse_meta, rules_version,
    },
    auth::RequireStudent,
    chat::{Conversation, Message, load_messages},
    error::ApiError,
    mutex::lock,
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/conversations/{id}/stream", get(stream))
}

/// 事件通道不設上限：一則回覆很小，且這樣慢速客戶端不會拖住 AI 的逾時計時。
type EventTx = mpsc::UnboundedSender<Event>;

/// 這個服務程序內正在產生的回覆：已送出的文字與正在收看的連線。
/// 學生離開對話頁再回來時，`GET /stream` 從這裡接上同一個回覆，而不是被擋下或重新產生。
///
/// 每次產生各有自己的紀錄（`LiveReply`），這裡只指向每場對話目前的那一個：舊回覆釋放 claim 後、
/// 送出最後事件前，新回覆可能已經開始並取代它，舊回覆結束時不能動到新回覆的紀錄。
/// 鎖的順序一律先這裡再 `Live`。
#[derive(Default)]
pub struct LiveReplies(Mutex<HashMap<Uuid, Arc<Mutex<Live>>>>);

struct Live {
    /// 已送出的回覆文字；新接上的連線先收到這一段
    text: String,
    watchers: Vec<EventTx>,
}

/// 一次回覆產生的紀錄，由產生回覆的工作持有。
pub(crate) struct LiveReply {
    id: Uuid,
    live: Arc<Mutex<Live>>,
}

impl LiveReply {
    /// 送出一段回覆給這次回覆的所有連線；已斷線的連線順便移除。
    fn delta(&self, text: &str) {
        let mut live = lock(&self.live);
        live.text.push_str(text);
        let ev = delta_event(text);
        live.watchers.retain(|w| w.send(ev.clone()).is_ok());
    }
}

impl LiveReplies {
    /// 這場對話的回覆是否正在產生（可以接上）。
    pub fn is_live(&self, id: Uuid) -> bool {
        lock(&self.0).contains_key(&id)
    }

    fn start(&self, id: Uuid, watcher: EventTx) -> LiveReply {
        let live = Arc::new(Mutex::new(Live {
            text: String::new(),
            watchers: vec![watcher],
        }));
        lock(&self.0).insert(id, live.clone());
        LiveReply { id, live }
    }

    /// 送出最後一個事件（done／error）並結束；之後的 `join` 不會再接上這次回覆。
    /// 只移除自己的紀錄：已被同一場對話的新回覆取代時，新回覆照常進行。
    fn finish(&self, reply: LiveReply, last: Event) {
        let mut replies = lock(&self.0);
        if replies
            .get(&reply.id)
            .is_some_and(|l| Arc::ptr_eq(l, &reply.live))
        {
            replies.remove(&reply.id);
        }
        for w in lock(&reply.live).watchers.drain(..) {
            let _ = w.send(last.clone());
        }
    }

    /// 接上進行中的回覆：先收到已產生的文字（一個 delta），之後與其他連線收到相同的事件。
    fn join(&self, id: Uuid) -> Option<mpsc::UnboundedReceiver<Event>> {
        let replies = lock(&self.0);
        let mut live = lock(replies.get(&id)?);
        let (tx, rx) = mpsc::unbounded_channel();
        if !live.text.is_empty() {
            let _ = tx.send(delta_event(&live.text));
        }
        live.watchers.push(tx);
        Some(rx)
    }
}

/// 嘗試取得這場對話的「正在產生回覆」權利；已有進行中的回覆時回傳 `false`。
pub async fn claim_generation(state: &AppState, id: Uuid) -> Result<bool, sqlx::Error> {
    let r = sqlx::query(concat!(
        "UPDATE conversations SET generating_since = now()
         WHERE id = $1 AND status = 'active' AND ",
        generation_unclaimed!(),
    ))
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
        tracing::error!(class = %crate::error::db_error_class(&e), "failed to release generation claim");
    }
}

pub type EventStream = Sse<KeepAliveStream<BoxStream<'static, Result<Event, Infallible>>>>;

/// 重試：學生訊息已保存但還沒有 AI 回覆時，再開一次串流。
#[utoipa::path(
    get,
    path = "/api/conversations/{id}/stream",
    tag = "conversations",
    summary = "重試或接回 AI 回覆（SSE）",
    description = "最後一則是學生訊息、還沒有 AI 回覆時重新產生；回覆正在產生時接上同一個回覆，第一個 delta 是目前已產生的全部文字。",
    params(("id" = Uuid, Path, description = "對話 ID")),
    responses((status = 200, description = "text/event-stream：delta {text}* 後接 done 或 error {code}", content_type = "text/event-stream", body = String), (status = 409, description = "classroom_archived / conversation_ended / reply_in_progress（回覆正在其他服務程序產生，無法接上）/ nothing_to_reply", body = crate::error::ErrorBody))
)]
pub(crate) async fn stream(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<EventStream, ApiError> {
    let conversation = {
        let mut conn = state.pool.acquire().await?;
        crate::classrooms::writable_conversation(&mut conn, cu.user.id, id).await?
    };
    if conversation.status != "active" {
        return Err(ApiError::conflict("conversation_ended", "對話已結束"));
    }
    if !claim_generation(&state, id).await? {
        // 回覆正在產生：接上同一個回覆
        if let Some(rx) = state.live.join(id) {
            return Ok(event_stream(rx));
        }
        // 回覆可能剛好在 claim 與 join 之間結束：再 claim 一次，由下面決定重新產生或 nothing_to_reply
        if !claim_generation(&state, id).await? {
            return Err(ApiError::conflict("reply_in_progress", "AI 正在回覆中"));
        }
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
    Ok(spawn_stream(
        state,
        cu.user.id,
        conversation,
        messages,
        None,
    ))
}

/// 學生訊息剛保存、且呼叫端已取得產生權利（`generating_since`）時，開始串流 AI 回覆。
/// `conversation` 須已反映這一輪的回合數與階段；`first` 是串流的第一個事件（保存好的學生訊息）。
/// 載入失敗時釋放 claim 並回傳錯誤。
pub async fn begin_stream(
    state: AppState,
    user: Uuid,
    conversation: Conversation,
    first: Event,
) -> Result<EventStream, ApiError> {
    match load_messages(&state, conversation.id).await {
        Ok(messages) => Ok(spawn_stream(
            state,
            user,
            conversation,
            messages,
            Some(first),
        )),
        Err(e) => {
            release_generation(&state, conversation.id).await;
            Err(e)
        }
    }
}

/// 只送出一個事件就結束的串流（例如達回合上限、沒有 AI 回覆時）。
pub fn single_event(first: Event) -> EventStream {
    Sse::new(futures_util::stream::once(async move { Ok(first) }).boxed())
        .keep_alive(KeepAlive::default())
}

fn spawn_stream(
    state: AppState,
    user: Uuid,
    conversation: Conversation,
    messages: Vec<Message>,
    first: Option<Event>,
) -> EventStream {
    let (tx, rx) = mpsc::unbounded_channel();
    if let Some(first) = first {
        let _ = tx.send(first);
    }
    let id = conversation.id;
    let reply = state.live.start(id, tx);
    tokio::spawn(
        async move {
            // 每位使用者的 AI 用量上限：用完時學生訊息已保存，但不呼叫 AI，以 error 事件結束（之後可重試）
            let last = match state.charge_ai(user) {
                Err(_) => event("error", json!({"code": crate::ai_usage::EXCEEDED_CODE})),
                Ok(slot) => {
                    let result =
                        AssertUnwindSafe(generate(&state, &conversation, messages, &reply))
                            .catch_unwind()
                            .await;
                    match result {
                        Ok(Ok(done)) => done,
                        // 沒有得到回覆：不扣額度，AI 當機時的重試不該把學生鎖在外面
                        Ok(Err(error)) => {
                            state.refund_ai(user, slot);
                            error
                        }
                        Err(_) => {
                            state.refund_ai(user, slot);
                            event("error", json!({"code": "internal"}))
                        }
                    }
                }
            };
            // 先釋放再送出最後一個事件：收到 done 後立刻送下一則訊息時，不會撞上還沒釋放的 claim
            release_generation(&state, id).await;
            state.live.finish(reply, last);
        }
        .in_current_span(),
    );
    event_stream(rx)
}

fn event_stream(rx: mpsc::UnboundedReceiver<Event>) -> EventStream {
    Sse::new(UnboundedReceiverStream::new(rx).map(Ok).boxed()).keep_alive(KeepAlive::default())
}

pub fn event(name: &str, data: serde_json::Value) -> Event {
    Event::default().event(name).data(data.to_string())
}

/// 每個片段都會送一次：直接序列化進事件，不經過 `serde_json::Value`。
fn delta_event(text: &str) -> Event {
    #[derive(Serialize)]
    struct Delta<'a> {
        text: &'a str,
    }
    Event::default()
        .event("delta")
        .json_data(Delta { text })
        .expect("a string always serializes")
}

/// 產生並保存回覆，片段經 `reply` 送給所有連線；回傳最後一個事件（done／error）。
async fn generate(
    state: &AppState,
    conv: &Conversation,
    messages: Vec<Message>,
    reply: &LiveReply,
) -> Result<Event, Event> {
    let req = AiRequest {
        system: build_system_prompt(&PromptContext {
            title: &conv.title,
            description: &conv.description,
            stage: conv.stage,
            turn: conv.turn_count,
            wrap_up_turn: state.config.wrap_up_turn,
            language: &conv.language,
        }),
        messages: messages
            .into_iter()
            .map(|m| ChatTurn {
                role: ChatRole::from_db_role(&m.role),
                content: m.content,
            })
            .collect(),
    };

    // 自動重試 1 次，但只在尚未串流出任何內容給學生時（S-03.5）
    let mut outcome = None;
    for _ in 0..2 {
        match attempt(state, reply, &req).await {
            Ok(r) => {
                outcome = Some(r);
                break;
            }
            Err(emitted_any) if emitted_any => break,
            Err(_) => {}
        }
    }
    let Some((text, meta)) = outcome else {
        return Err(event("error", json!({"code": "ai_unavailable"})));
    };

    match save_reply(state, conv, &text, &meta).await {
        Ok(Some(saved)) => Ok(event(
            "done",
            json!({
                "message_id": saved.message_id,
                "question_type": meta.question_type,
                "stage": saved.stage,
                "suggest_end": saved.converge_ready,
            }),
        )),
        // 對話在串流期間已結束或已有回覆
        Ok(None) => Err(event("error", json!({"code": "conflict"}))),
        Err(e) => {
            tracing::error!(class = %crate::error::db_error_class(&e), "failed to save AI reply");
            Err(event("error", json!({"code": "internal"})))
        }
    }
}

/// 一次 AI 呼叫。失敗時回傳「是否已經送出內容給學生」，決定能不能自動重試。
async fn attempt(
    state: &AppState,
    reply: &LiveReply,
    req: &AiRequest,
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
                // 空片段（模型開始推理）也算首個 token，但沒有文字可送
                got_chunk = true;
                let out = splitter.push(&chunk);
                if !out.is_empty() {
                    emitted = true;
                    text.push_str(&out);
                    reply.delta(&out);
                }
            }
        }
    }
    let (rest, meta_raw) = splitter.finish();
    if !rest.is_empty() {
        emitted = true;
        text.push_str(&rest);
        reply.delta(&rest);
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
    // 階段只進不退；第 3 階段達成條件即符合收斂條件（S-03.2、S-03.3）
    let new_stage = if meta.advance && stage < 3 {
        stage + 1
    } else {
        stage
    };
    let converge_ready = ready || (stage == 3 && meta.advance);
    // 保存回覆與更新階段合成一個語句，少一次資料庫往返
    let (message_id,): (Uuid,) = sqlx::query_as(
        "WITH m AS (
             INSERT INTO messages (conversation_id, role, content, rules_version, question_type,
                                   advance_reason)
             VALUES ($1, 'ai', $2, $3, $4, $5) RETURNING id
         ), c AS (
             UPDATE conversations SET stage = $6, converge_ready = $7 WHERE id = $1
         )
         SELECT id FROM m",
    )
    .bind(conv.id)
    .bind(text)
    .bind(rules_version(&conv.language))
    .bind(&meta.question_type)
    .bind(&meta.reason)
    .bind(new_stage)
    .bind(converge_ready)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Some(Saved {
        message_id,
        stage: new_stage,
        converge_ready,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::error::TryRecvError;

    #[test]
    fn a_replaced_reply_finishes_only_itself() {
        let replies = LiveReplies::default();
        let id = Uuid::new_v4();
        let (old_tx, mut old_rx) = mpsc::unbounded_channel();
        let (new_tx, mut new_rx) = mpsc::unbounded_channel();
        let old = replies.start(id, old_tx);
        // 舊回覆釋放 claim 之後、送出最後事件之前，新回覆已開始
        let new = replies.start(id, new_tx);

        replies.finish(old, event("error", json!({"code": "ai_unavailable"})));
        // 舊回覆的連線收到自己的最後事件後結束
        assert!(old_rx.try_recv().is_ok());
        assert_eq!(old_rx.try_recv().unwrap_err(), TryRecvError::Disconnected);
        // 新回覆不受影響：仍可接上，自己的連線也還開著
        assert!(replies.is_live(id));
        assert_eq!(new_rx.try_recv().unwrap_err(), TryRecvError::Empty);

        new.delta("嗨");
        assert!(new_rx.try_recv().is_ok());
        let mut joined = replies.join(id).expect("the new reply is still joinable");
        assert!(joined.try_recv().is_ok());
        replies.finish(new, event("done", json!({})));
        assert!(!replies.is_live(id));
        assert!(new_rx.try_recv().is_ok());
        assert!(joined.try_recv().is_ok());
    }
}
