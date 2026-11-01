//! 伺服器端語音對話：把 sglang-omni 的 Streaming Cascade 接到現有的對話流程。預設關閉。
//!
//! 瀏覽器只連 `GET /api/conversations/{id}/voice`，這裡檢查身分後轉送到 cascade 的
//! `/v1/realtime`。cascade 辨識出學生的話後呼叫內部的 `/v1/chat/completions`，
//! 由這裡走與文字相同的流程：存訊息、扣用量、組提示、遮蔽個資、呼叫 OpenAI、存回覆。
//! cascade 自己的 system prompt 與歷史不會送給 OpenAI。
//!
//! cascade 沒有逐會話傳身分的管道，所以用 `instructions` 帶一次性代號：它會原樣成為
//! system message 的開頭。這個值只有後端知道，瀏覽器不能送 `session.update`，
//! 也看不到回傳的 `session.*`。
//!
//! 音訊只轉送、不保存，也不寫日誌（S-05.4）。音訊上傳已由需求方接受（S-05.5）。

use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{
        Path, State,
        rejection::JsonRejection,
        ws::{
            Message as ClientFrame, WebSocket, WebSocketUpgrade,
            rejection::WebSocketUpgradeRejection,
        },
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt, stream};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex as TurnLock, mpsc};
use tokio_tungstenite::tungstenite::Message as CascadeFrame;
use uuid::Uuid;

use crate::{
    AppState,
    auth::RequireStudent,
    chat::{MAX_MESSAGE_CHARS, Submitted, submit_message},
    error::ApiError,
    mutex::lock,
    reply::{ReplyText, begin_text_stream, save_heard},
};

pub const INTERNAL_PREFIX: &str = "/internal/cascade/";
const SESSION_PREFIX: &str = "voice-session:";
/// 一幀 100 ms 的 16 kHz 音訊約 5 KB；上限只是擋住異常的訊息。
const MAX_FRAME_BYTES: usize = 256 * 1024;
/// 學生打斷時上一則回覆可能還在產生，等它存好才能收下一句。
const REPLY_WAIT_ATTEMPTS: u32 = 50;
const REPLY_WAIT_STEP: Duration = Duration::from_millis(200);
const SOURCE: &str = "speech-cascade";
const ENDED_REPLY: &str = "這次的討論到這裡告一段落，謝謝你。";
const FAILED_REPLY: &str = "抱歉，我這邊出了點問題，語音對話先停在這裡。";
/// 這一輪沒有成功：瀏覽器要結束語音，讓學生重試。這是後端自己發的，不經過 cascade。
const TURN_FAILED: &str = "{\"type\":\"socrates.turn.failed\"}";

const UNAVAILABLE: ApiError = ApiError::new(
    StatusCode::SERVICE_UNAVAILABLE,
    "voice_unavailable",
    "語音服務尚未啟用",
);

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/conversations/{id}/voice", get(socket))
        .route("/internal/cascade/v1/models", get(models))
        .route("/internal/cascade/v1/chat/completions", post(chat))
}

/// 放進 `instructions` 的文字，也是 system message 的開頭。
pub fn voice_session_prompt(token: &str) -> String {
    format!("{SESSION_PREFIX}{token}")
}

/// 瀏覽器可以送給 cascade 的事件。`session.update` 能改掉會話代號，一律不放行。
pub fn allow_client_event(kind: &str) -> bool {
    matches!(
        kind,
        "input_audio_buffer.append" | "response.cancel" | "conversation.item.truncate"
    )
}

/// 可以回給瀏覽器的事件：`session.created`／`session.updated` 帶著會話代號。
pub fn allow_server_event(kind: &str) -> bool {
    !matches!(kind, "session.created" | "session.updated")
}

struct Live {
    user: Uuid,
    conversation: Uuid,
    /// cascade 上一次送來、已保存的學生發言與歷史中的學生訊息數
    spoken: String,
    user_turns: usize,
    /// 同一場一次處理一句：比對上一句到保存之間不能插入別的請求
    turn: Arc<TurnLock<()>>,
    notices: mpsc::UnboundedSender<String>,
}

#[derive(Default)]
struct Sessions {
    by_token: HashMap<String, Live>,
    users: HashSet<Uuid>,
}

/// 進行中的語音對話：一次性代號對應到學生與對話。只在記憶體，多個實例各自計算。
#[derive(Default)]
pub struct VoiceSessions(Mutex<Sessions>);

impl VoiceSessions {
    /// 一位學生同時一場；已有進行中的就回 `None`。
    pub fn open(self: &Arc<Self>, user: Uuid, conversation: Uuid) -> Option<VoiceSession> {
        let mut sessions = lock(&self.0);
        if !sessions.users.insert(user) {
            return None;
        }
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let (notices, notices_rx) = mpsc::unbounded_channel();
        sessions.by_token.insert(
            token.clone(),
            Live {
                user,
                conversation,
                spoken: String::new(),
                user_turns: 0,
                turn: Arc::default(),
                notices,
            },
        );
        Some(VoiceSession {
            sessions: self.clone(),
            token,
            user,
            conversation,
            notices: Some(notices_rx),
        })
    }

    /// 代號對應的（學生、對話）。
    pub fn lookup(&self, token: &str) -> Option<(Uuid, Uuid)> {
        lock(&self.0)
            .by_token
            .get(token)
            .map(|live| (live.user, live.conversation))
    }

    fn turn_lock(&self, token: &str) -> Option<Arc<TurnLock<()>>> {
        lock(&self.0)
            .by_token
            .get(token)
            .map(|live| live.turn.clone())
    }

    /// 告訴瀏覽器這一輪失敗；會話已結束時沒有人要聽。
    fn turn_failed(&self, token: &str) {
        if let Some(live) = lock(&self.0).by_token.get(token) {
            let _ = live.notices.send(TURN_FAILED.to_string());
        }
    }

    /// 這次發言中還沒存過的部分。回覆一個字都沒被聽到時，cascade 會移除回覆，把前後兩句以空格
    /// 併成一則，學生訊息數也就不會增加；兩個條件都成立才去掉舊的部分，學生重複自己的話不受影響。
    pub fn new_speech(&self, token: &str, content: &str, user_turns: usize) -> String {
        let sessions = lock(&self.0);
        let Some(live) = sessions.by_token.get(token) else {
            return content.to_string();
        };
        (user_turns <= live.user_turns && !live.spoken.is_empty())
            .then(|| content.strip_prefix(live.spoken.as_str()))
            .flatten()
            .and_then(|rest| rest.strip_prefix(' '))
            .filter(|rest| !rest.is_empty())
            .unwrap_or(content)
            .to_string()
    }

    /// 學生訊息保存成功後才記下：沒存到的話不能在下次比對時被當成存過而去掉。
    fn remember(&self, token: &str, content: &str, user_turns: usize) {
        if let Some(live) = lock(&self.0).by_token.get_mut(token) {
            live.spoken = content.to_string();
            live.user_turns = user_turns;
        }
    }
}

/// 連線結束時釋放，代號也跟著失效。
pub struct VoiceSession {
    sessions: Arc<VoiceSessions>,
    token: String,
    user: Uuid,
    conversation: Uuid,
    notices: Option<mpsc::UnboundedReceiver<String>>,
}

impl VoiceSession {
    pub fn token(&self) -> &str {
        &self.token
    }

    /// 後端要主動告訴瀏覽器的事件；只能取走一次。
    pub fn take_notices(&mut self) -> Option<mpsc::UnboundedReceiver<String>> {
        self.notices.take()
    }
}

/// cascade 的事件裡學生聽到的字元數。回覆取消時先算沒聽到，瀏覽器回報過播放位置才會收到
/// `heard`，它會蓋掉前者；回覆在沒有任何聲音時被取消就不會有 `heard`。
pub fn heard_chars(event: &serde_json::Value) -> Option<usize> {
    match event["type"].as_str()? {
        "sglang.response.heard" => event["text_end"].as_u64().map(|n| n as usize),
        "response.done" if event["response"]["status"] == "cancelled" => Some(0),
        _ => None,
    }
}

/// 記下學生聽到多少；回覆還在產生時等它存好再寫。
pub async fn record_heard(state: &AppState, conversation: Uuid, chars: usize) {
    if !state.live.cut(conversation, chars) {
        save_heard(state, conversation, chars).await;
    }
}

impl Drop for VoiceSession {
    fn drop(&mut self) {
        let mut sessions = lock(&self.sessions.0);
        sessions.by_token.remove(&self.token);
        sessions.users.remove(&self.user);
    }
}

/// 建立語音對話的 WebSocket；訊息格式是 cascade 的 realtime 事件，只放行音訊相關事件。
#[utoipa::path(
    get,
    path = "/api/conversations/{id}/voice",
    tag = "conversations",
    summary = "語音對話 WebSocket（實驗，預設關閉）",
    params(("id" = Uuid, Path, description = "對話 ID")),
    responses(
        (status = 101, description = "升級為 WebSocket"),
        (status = 403, description = "csrf：Origin 不是本站", body = crate::error::ErrorBody),
        (status = 409, description = "conversation_ended / voice_session_active：已有進行中的語音對話", body = crate::error::ErrorBody),
        (status = 503, description = "voice_unavailable", body = crate::error::ErrorBody)
    )
)]
pub(crate) async fn socket(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Result<Response, ApiError> {
    let Some(url) = state.config.cascade_realtime_url.clone() else {
        return Err(UNAVAILABLE);
    };
    let conversation = {
        let mut conn = state.pool.acquire().await?;
        crate::classrooms::writable_conversation(&mut conn, cu.user.id, id).await?
    };
    if conversation.status != "active" {
        return Err(ApiError::conflict("conversation_ended", "對話已結束"));
    }
    // WebSocket 升級是 GET，全站的 Origin 檢查不會擋，這裡自己擋跨站連線
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    if origin != Some(state.config.app_base_url.as_str()) {
        return Err(ApiError::forbidden("csrf", "請求來源不被允許"));
    }
    let upgrade = match upgrade {
        Ok(upgrade) => upgrade,
        Err(rejection) => return Ok(rejection.into_response()),
    };
    let session = state
        .voice_sessions
        .open(cu.user.id, id)
        .ok_or(ApiError::conflict(
            "voice_session_active",
            "已有進行中的語音對話",
        ))?;
    Ok(upgrade
        .max_message_size(MAX_FRAME_BYTES)
        .on_upgrade(move |client| relay(client, url, session, state)))
}

fn parse_event(text: &str) -> Option<serde_json::Value> {
    serde_json::from_str(text).ok()
}

fn event_kind(event: &serde_json::Value) -> Option<&str> {
    event["type"].as_str()
}

/// 兩邊互相轉送到其中一邊斷線。內容一律不寫日誌。
async fn relay(client: WebSocket, url: String, mut session: VoiceSession, state: AppState) {
    let Some(mut notices) = session.take_notices() else {
        return;
    };
    let Ok((cascade, _)) = tokio_tungstenite::connect_async(url.as_str()).await else {
        tracing::warn!("cannot reach the voice cascade");
        return;
    };
    let (mut to_cascade, mut from_cascade) = cascade.split();
    let (mut to_client, mut from_client) = client.split();
    let configure = json!({
        "type": "session.update",
        "event_id": Uuid::new_v4().simple().to_string(),
        "session": {
            "type": "realtime",
            "instructions": voice_session_prompt(session.token()),
            "audio": {"input": {"format": {"type": "audio/pcm", "rate": 16000}}},
        },
    });
    if to_cascade
        .send(CascadeFrame::text(configure.to_string()))
        .await
        .is_err()
    {
        return;
    }
    loop {
        tokio::select! {
            frame = from_client.next() => match frame {
                Some(Ok(ClientFrame::Text(text))) => {
                    let allowed = parse_event(&text)
                        .is_some_and(|e| event_kind(&e).is_some_and(allow_client_event));
                    if allowed && to_cascade.send(CascadeFrame::text(text.as_str())).await.is_err() {
                        break;
                    }
                }
                Some(Ok(ClientFrame::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
            frame = from_cascade.next() => match frame {
                Some(Ok(CascadeFrame::Text(text))) => {
                    let event = parse_event(&text);
                    if let Some(chars) = event.as_ref().and_then(heard_chars) {
                        record_heard(&state, session.conversation, chars).await;
                    }
                    let allowed = event
                        .as_ref()
                        .is_some_and(|e| event_kind(e).is_some_and(allow_server_event));
                    if allowed && to_client.send(ClientFrame::text(text.as_str())).await.is_err() {
                        break;
                    }
                }
                Some(Ok(CascadeFrame::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
            Some(notice) = notices.recv() => {
                if to_client.send(ClientFrame::text(notice)).await.is_err() {
                    break;
                }
            }
        }
    }
    let _ = to_cascade.send(CascadeFrame::Close(None)).await;
    let _ = to_client.send(ClientFrame::Close(None)).await;
}

/// 比對雜湊，比較時間與密鑰內容無關。
fn check_secret(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let Some(secret) = state.config.cascade_llm_token.as_deref() else {
        return Err(ApiError::not_found());
    };
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(ApiError::unauthorized())?;
    if Sha256::digest(given.as_bytes()) == Sha256::digest(secret.as_bytes()) {
        Ok(())
    } else {
        Err(ApiError::unauthorized())
    }
}

async fn models(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    check_secret(&state, &headers)?;
    Ok(
        Json(json!({"object": "list", "data": [{"id": "socrates", "object": "model"}]}))
            .into_response(),
    )
}

#[derive(Deserialize)]
struct ChatBody {
    messages: Vec<ChatMessage>,
}

#[derive(Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

fn delta_chunk(text: &str) -> Bytes {
    let chunk = json!({"choices": [{"delta": {"content": text}, "finish_reason": null}]});
    Bytes::from(format!("data: {chunk}\n\n"))
}

const END_OF_REPLY: &str =
    "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";

fn sse(body: Body) -> Response {
    ([(header::CONTENT_TYPE, "text/event-stream")], body).into_response()
}

fn fixed_reply(text: &'static str) -> Response {
    let chunks = [
        delta_chunk(text),
        Bytes::from_static(END_OF_REPLY.as_bytes()),
    ];
    sse(Body::from_stream(stream::iter(
        chunks.map(Ok::<_, Infallible>),
    )))
}

/// 把回覆文字轉成 OpenAI 串流，失敗時通知瀏覽器；一個字都沒有時改唸固定句子，cascade 才不會
/// 等到逾時。cascade 中途放棄這個串流時（打斷）不會有通知，下一輪保存學生訊息時會發現。
fn streamed_reply(
    text: mpsc::UnboundedReceiver<ReplyText>,
    state: AppState,
    token: String,
) -> Response {
    let begin = (text, state, token, false, false);
    let chunks = stream::unfold(
        begin,
        |(mut text, state, token, mut sent, done)| async move {
            if done {
                return None;
            }
            loop {
                match text.recv().await {
                    Some(ReplyText::Delta(delta)) => {
                        return Some((delta_chunk(&delta), (text, state, token, true, false)));
                    }
                    Some(ReplyText::Failed) => state.voice_sessions.turn_failed(&token),
                    None if sent => {
                        let end = Bytes::from_static(END_OF_REPLY.as_bytes());
                        return Some((end, (text, state, token, true, true)));
                    }
                    None => {
                        sent = true;
                        return Some((
                            delta_chunk(FAILED_REPLY),
                            (text, state, token, sent, false),
                        ));
                    }
                }
            }
        },
    );
    sse(Body::from_stream(chunks.map(Ok::<_, Infallible>)))
}

/// cascade 的 LLM 節點：學生的一句話走與文字相同的回覆流程。
async fn chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<ChatBody>, JsonRejection>,
) -> Result<Response, ApiError> {
    check_secret(&state, &headers)?;
    let Json(body) = body.map_err(|_| ApiError::bad_request("invalid_message", "訊息長度不符"))?;
    let token = body
        .messages
        .iter()
        .find(|m| m.role == "system")
        .and_then(|m| m.content.strip_prefix(SESSION_PREFIX))
        .and_then(|rest| rest.split_whitespace().next());
    // cascade 啟動時會試送一次，那時還沒有會話
    let Some(token) = token else {
        return Ok(fixed_reply("好"));
    };
    let token = token.to_string();
    let (user, id) = state
        .voice_sessions
        .lookup(&token)
        .ok_or(ApiError::not_found())?;
    let turn = spoken_turn(&state, &token, user, id, &body.messages).await;
    if turn.is_err() {
        state.voice_sessions.turn_failed(&token);
    }
    turn
}

/// 保存學生這句話並開始回覆。
async fn spoken_turn(
    state: &AppState,
    token: &str,
    user: Uuid,
    id: Uuid,
    messages: &[ChatMessage],
) -> Result<Response, ApiError> {
    let content = messages
        .iter()
        .rfind(|m| m.role == "user")
        .map(|m| m.content.trim())
        .unwrap_or_default();
    if content.is_empty() || content.chars().count() > MAX_MESSAGE_CHARS {
        return Err(ApiError::bad_request("invalid_message", "訊息長度不符"));
    }
    let user_turns = messages.iter().filter(|m| m.role == "user").count();
    let turn_lock = state
        .voice_sessions
        .turn_lock(token)
        .ok_or(ApiError::not_found())?;
    let _turn = turn_lock.lock().await;
    let fresh = state.voice_sessions.new_speech(token, content, user_turns);
    let mut attempts = 0;
    let submitted = loop {
        match submit_message(state, user, id, &fresh, SOURCE, true).await {
            Err(e) if e.code == "reply_in_progress" && attempts < REPLY_WAIT_ATTEMPTS => {
                attempts += 1;
                tokio::time::sleep(REPLY_WAIT_STEP).await;
            }
            result => break result?,
        }
    };
    state.voice_sessions.remember(token, content, user_turns);
    match submitted {
        Submitted::Ended(_) => Ok(fixed_reply(ENDED_REPLY)),
        Submitted::Reply {
            conversation,
            first,
        } => {
            let text = begin_text_stream(state.clone(), user, conversation, first).await?;
            Ok(streamed_reply(text, state.clone(), token.to_string()))
        }
    }
}
