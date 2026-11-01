//! 語音對話實驗：把 sglang-omni 的 Streaming Cascade 接到現有的對話流程。預設關閉。
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
//! 音訊只轉送、不保存，也不寫日誌（S-05.4）。這條路會把音訊送到伺服器，與 S-05.5
//! 「開口偵測只在瀏覽器內」不同，規格尚未修訂，不能直接啟用給學生。

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
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as CascadeFrame;
use uuid::Uuid;

use crate::{
    AppState,
    auth::RequireStudent,
    chat::{MAX_MESSAGE_CHARS, Submitted, submit_message},
    error::ApiError,
    mutex::lock,
    reply::begin_text_stream,
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
const FAILED_REPLY: &str = "抱歉，我這邊出了點問題，請再說一次。";

const UNAVAILABLE: ApiError = ApiError::new(
    StatusCode::SERVICE_UNAVAILABLE,
    "voice_unavailable",
    "語音服務尚未啟用",
);

pub fn routes() -> Router<AppState> {
    let internal = |path: &str| format!("{INTERNAL_PREFIX}v1/{path}");
    Router::new()
        .route("/api/conversations/{id}/voice", get(socket))
        .route(&internal("models"), get(models))
        .route(&internal("chat/completions"), post(chat))
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

#[derive(Default)]
struct Sessions {
    by_token: HashMap<String, (Uuid, Uuid)>,
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
        sessions
            .by_token
            .insert(token.clone(), (user, conversation));
        Some(VoiceSession {
            sessions: self.clone(),
            token,
            user,
        })
    }

    /// 代號對應的（學生、對話）。
    pub fn lookup(&self, token: &str) -> Option<(Uuid, Uuid)> {
        lock(&self.0).by_token.get(token).copied()
    }
}

/// 連線結束時釋放，代號也跟著失效。
pub struct VoiceSession {
    sessions: Arc<VoiceSessions>,
    token: String,
    user: Uuid,
}

impl VoiceSession {
    pub fn token(&self) -> &str {
        &self.token
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
        .on_upgrade(move |client| relay(client, url, session)))
}

fn event_kind(text: &str) -> Option<String> {
    let event: serde_json::Value = serde_json::from_str(text).ok()?;
    event["type"].as_str().map(String::from)
}

/// 兩邊互相轉送到其中一邊斷線。內容一律不寫日誌。
async fn relay(client: WebSocket, url: String, session: VoiceSession) {
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
                    let allowed = event_kind(&text).is_some_and(|kind| allow_client_event(&kind));
                    if allowed && to_cascade.send(CascadeFrame::text(text.as_str())).await.is_err() {
                        break;
                    }
                }
                Some(Ok(ClientFrame::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
            frame = from_cascade.next() => match frame {
                Some(Ok(CascadeFrame::Text(text))) => {
                    let allowed = event_kind(&text).is_some_and(|kind| allow_server_event(&kind));
                    if allowed && to_client.send(ClientFrame::text(text.as_str())).await.is_err() {
                        break;
                    }
                }
                Some(Ok(CascadeFrame::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
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

/// 把回覆文字轉成 OpenAI 串流；AI 失敗、一個字都沒有時改唸固定句子，cascade 才不會等到逾時。
fn streamed_reply(text: mpsc::UnboundedReceiver<String>) -> Response {
    let chunks = stream::unfold((text, false, false), |(mut text, sent, done)| async move {
        if done {
            return None;
        }
        match text.recv().await {
            Some(delta) => Some((delta_chunk(&delta), (text, true, false))),
            None if sent => Some((
                Bytes::from_static(END_OF_REPLY.as_bytes()),
                (text, true, true),
            )),
            None => Some((delta_chunk(FAILED_REPLY), (text, true, false))),
        }
    });
    sse(Body::from_stream(chunks.map(Ok::<_, Infallible>)))
}

/// cascade 的 LLM 節點：學生的一句話走與文字相同的回覆流程。
async fn chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<ChatBody>, JsonRejection>,
) -> Result<Response, ApiError> {
    check_secret(&state, &headers)?;
    let Json(body) =
        body.map_err(|_| ApiError::bad_request("invalid_message", "訊息長度不符"))?;
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
    let (user, id) = state
        .voice_sessions
        .lookup(token)
        .ok_or(ApiError::not_found())?;
    let content = body
        .messages
        .iter()
        .rfind(|m| m.role == "user")
        .map(|m| m.content.trim())
        .unwrap_or_default();
    if content.is_empty() || content.chars().count() > MAX_MESSAGE_CHARS {
        return Err(ApiError::bad_request("invalid_message", "訊息長度不符"));
    }
    let mut attempts = 0;
    let submitted = loop {
        match submit_message(&state, user, id, content, SOURCE).await {
            Err(e) if e.code == "reply_in_progress" && attempts < REPLY_WAIT_ATTEMPTS => {
                attempts += 1;
                tokio::time::sleep(REPLY_WAIT_STEP).await;
            }
            result => break result?,
        }
    };
    match submitted {
        Submitted::Ended(_) => Ok(fixed_reply(ENDED_REPLY)),
        Submitted::Reply {
            conversation,
            first,
        } => {
            let text = begin_text_stream(state, user, conversation, first).await?;
            Ok(streamed_reply(text))
        }
    }
}
