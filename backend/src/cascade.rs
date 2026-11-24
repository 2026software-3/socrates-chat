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
    time::{Duration, Instant},
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
    chat::{MAX_MESSAGE_CHARS, Submitted, rejoin_message, submit_message},
    error::ApiError,
    mutex::lock,
    reply::{Abandoned, ReplyText, begin_text_stream, save_heard},
    voice_nodes::{CONNECT_TIMEOUT, Node, NodeId},
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
const UNAVAILABLE: ApiError = ApiError::new(
    StatusCode::SERVICE_UNAVAILABLE,
    "voice_unavailable",
    "語音服務尚未啟用",
);

/// 瀏覽器的入口。
pub fn routes() -> Router<AppState> {
    Router::new().route("/api/conversations/{id}/voice", get(socket))
}

/// cascade 回呼的內部端點。設了 `INTERNAL_LISTEN_ADDR` 時由獨立的監聽器提供，不在公開 router（S-19）。
pub fn internal_routes() -> Router<AppState> {
    Router::new()
        .route("/internal/cascade/v1/models", get(models))
        .route("/internal/cascade/v1/chat/completions", post(chat))
}

/// 進行中的場次多久檢查一次所屬節點是否已被停用或刪除；加上節點清單的快取時間，
/// 最慢約 2.5 秒內結束。
const REVOKE_CHECK: Duration = Duration::from_millis(500);

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
    /// 這場綁定的節點；內部端點用它確認呼叫者的密鑰屬於同一個節點
    node: NodeId,
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
    /// 各節點進行中的場次數
    counts: HashMap<NodeId, usize>,
}

impl Sessions {
    /// 在候選裡選場次最少、未滿的節點；選擇與加一必須在同一把鎖內（呼叫端持有鎖）。
    fn pick<'a>(&self, candidates: &'a [Node]) -> Option<&'a Node> {
        let count = |n: &Node| self.counts.get(&n.id).copied().unwrap_or(0);
        candidates
            .iter()
            .filter(|n| n.max_sessions.is_none_or(|max| count(n) < max))
            .min_by(|a, b| a.pick_key(count(a)).cmp(&b.pick_key(count(b))))
    }

    fn release(&mut self, node: NodeId) {
        if let Some(n) = self.counts.get_mut(&node) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                self.counts.remove(&node);
            }
        }
    }
}

/// 開啟語音場次失敗的原因。
#[derive(Debug, PartialEq, Eq)]
pub enum OpenError {
    /// 這位學生已有進行中的場次
    UserBusy,
    /// 沒有可用的節點（全部不健康、暫停或已滿）
    NoNode,
}

/// 進行中的語音對話：一次性代號對應到學生與對話。只在記憶體，多個實例各自計算。
#[derive(Default)]
pub struct VoiceSessions(Mutex<Sessions>);

impl VoiceSessions {
    /// 一位學生同時一場；已有進行中的就回 `None`。綁在環境節點、不受場次上限限制。
    pub fn open(self: &Arc<Self>, user: Uuid, conversation: Uuid) -> Option<VoiceSession> {
        let mut sessions = lock(&self.0);
        sessions.users.insert(user).then(|| {
            self.insert(
                &mut sessions,
                user,
                conversation,
                NodeId::Env,
                String::new(),
            )
        })
    }

    /// 依 S-19 的規則選節點並開啟場次：選擇與場次加一在同一把鎖內完成，併發不會超過上限。
    pub fn open_balanced(
        self: &Arc<Self>,
        user: Uuid,
        conversation: Uuid,
        candidates: &[Node],
    ) -> Result<VoiceSession, OpenError> {
        let mut sessions = lock(&self.0);
        if sessions.users.contains(&user) {
            return Err(OpenError::UserBusy);
        }
        let node = sessions.pick(candidates).ok_or(OpenError::NoNode)?;
        let (id, url) = (node.id, node.realtime_url.clone());
        sessions.users.insert(user);
        Ok(self.insert(&mut sessions, user, conversation, id, url))
    }

    fn insert(
        self: &Arc<Self>,
        sessions: &mut Sessions,
        user: Uuid,
        conversation: Uuid,
        node: NodeId,
        url: String,
    ) -> VoiceSession {
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let (notices, notices_rx) = mpsc::unbounded_channel();
        *sessions.counts.entry(node).or_insert(0) += 1;
        sessions.by_token.insert(
            token.clone(),
            Live {
                user,
                conversation,
                node,
                spoken: String::new(),
                user_turns: 0,
                turn: Arc::default(),
                notices,
            },
        );
        VoiceSession {
            sessions: self.clone(),
            token,
            user,
            conversation,
            node,
            url,
            notices: Some(notices_rx),
        }
    }

    /// 連不上節點時改綁候選中的另一個（呼叫端已排除失敗者）。沒有可選的就維持原綁定並回 `false`，
    /// 場次數照常在 `VoiceSession` 釋放時扣回，不會洩漏。
    pub fn rebind(&self, session: &mut VoiceSession, candidates: &[Node]) -> bool {
        let mut sessions = lock(&self.0);
        let Some(node) = sessions.pick(candidates) else {
            return false;
        };
        let (id, url) = (node.id, node.realtime_url.clone());
        sessions.release(session.node);
        *sessions.counts.entry(id).or_insert(0) += 1;
        if let Some(live) = sessions.by_token.get_mut(&session.token) {
            live.node = id;
        }
        session.node = id;
        session.url = url;
        true
    }

    /// 各節點進行中的場次數。
    pub fn counts(&self) -> HashMap<NodeId, usize> {
        lock(&self.0).counts.clone()
    }

    /// 代號綁定的節點。
    pub fn node_of(&self, token: &str) -> Option<NodeId> {
        lock(&self.0).by_token.get(token).map(|live| live.node)
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

    /// 告訴瀏覽器這一輪沒有成功：瀏覽器要結束語音，讓學生重試。這是後端自己發的，不經過 cascade；
    /// 會話已結束時沒有人要聽。
    /// `code` 是學生端要顯示的原因（例如教室的 AI 不可用），沒有就是一般失敗。
    fn turn_failed(&self, token: &str, code: Option<&str>) {
        if let Some(live) = lock(&self.0).by_token.get(token) {
            let notice = match code {
                Some(code) => json!({"type": "socrates.turn.failed", "code": code}),
                None => json!({"type": "socrates.turn.failed"}),
            };
            let _ = live.notices.send(notice.to_string());
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
    node: NodeId,
    url: String,
    notices: Option<mpsc::UnboundedReceiver<String>>,
}

impl VoiceSession {
    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn node(&self) -> NodeId {
        self.node
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

/// 句內沒有時間資訊時用的語速，取自實測的 TTS（29 字約 7.9 秒）。
const CHARS_PER_SECOND: f64 = 3.5;
/// 少於這個字數聽不出內容，算沒聽到。
const MIN_HEARD_CHARS: usize = 4;

/// 算出學生聽到幾個字。cascade 的 `heard` 只到整句，蘇格拉底式回覆常常整則就是一句，
/// 所以用每句的進度與瀏覽器回報的播放位置，在句內按比例推算。
#[derive(Default)]
pub struct HeardTracker {
    /// 這則回覆到每句結尾的（字元數、音訊毫秒）
    progress: Vec<(usize, f64)>,
    played_ms: Option<f64>,
}

impl HeardTracker {
    pub fn from_client(&mut self, event: &serde_json::Value) {
        if event_kind(event) == Some("conversation.item.truncate") {
            self.played_ms = event["audio_end_ms"].as_f64();
        }
    }

    /// 這個事件代表學生聽到的字元數時回傳。
    pub fn from_cascade(&mut self, event: &serde_json::Value) -> Option<usize> {
        match event_kind(event)? {
            "response.created" => {
                *self = Self::default();
                None
            }
            "sglang.response.progress" => {
                let text_end = event["text_end"].as_u64()? as usize;
                self.progress.push((text_end, event["audio_ms"].as_f64()?));
                None
            }
            _ => heard_chars(event).map(|chars| match event_kind(event) {
                Some("sglang.response.heard") => chars.max(self.estimate()),
                _ => chars,
            }),
        }
    }

    fn estimate(&self) -> usize {
        let Some(played) = self.played_ms else {
            return 0;
        };
        let mut before = (0, 0.0);
        let mut chars = None;
        for &(text_end, audio_ms) in &self.progress {
            if played <= audio_ms {
                let part = (played - before.1) / (audio_ms - before.1).max(1.0);
                chars = Some(before.0 + (part * (text_end - before.0) as f64) as usize);
                break;
            }
            before = (text_end, audio_ms);
        }
        let chars = chars.unwrap_or_else(|| {
            before.0 + ((played - before.1) / 1000.0 * CHARS_PER_SECOND) as usize
        });
        if chars < MIN_HEARD_CHARS { 0 } else { chars }
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
        sessions.release(self.node);
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
    let candidates = state.voice_nodes.candidates(&state, &[]).await;
    if candidates.is_empty() {
        return Err(UNAVAILABLE);
    }
    let conversation = {
        let mut conn = state.pool.acquire().await?;
        crate::classrooms::writable_conversation(&mut conn, cu.user.id, id).await?
    };
    if conversation.status != "active" {
        return Err(ApiError::conversation_ended());
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
        .open_balanced(cu.user.id, id, &candidates)
        .map_err(|e| match e {
            OpenError::UserBusy => {
                ApiError::conflict("voice_session_active", "已有進行中的語音對話")
            }
            OpenError::NoNode => UNAVAILABLE,
        })?;
    Ok(upgrade
        .max_message_size(MAX_FRAME_BYTES)
        .on_upgrade(move |client| relay(client, session, state)))
}

/// 診斷日誌的一筆：瀏覽器與 cascade 之間的控制事件，只有種類、狀態與播放毫秒數。
/// 音訊、逐字稿、回覆文字與項目代號一律不放進來。
#[derive(Debug, PartialEq, Eq)]
pub struct VoiceTrace {
    pub from: &'static str,
    pub kind: &'static str,
    pub status: Option<&'static str>,
    pub audio_end_ms: Option<u64>,
}

/// 這個事件要不要留診斷日誌（時間由日誌本身帶）。用來分辨回覆被取消的原因：
/// 是 VAD 聽到聲音（`speech_started`）、瀏覽器主動取消或截斷，還是 cascade 自己結束。
/// 方向不對的同名事件不記，狀態只認 cascade 已知的幾種，其餘寫成 `other`。
pub fn voice_trace(from_client: bool, event: &serde_json::Value) -> Option<VoiceTrace> {
    let kind = event_kind(event)?;
    let (from, kind, status, audio_end_ms) = match (from_client, kind) {
        (true, "response.cancel") => ("client", "response.cancel", None, None),
        (true, "conversation.item.truncate") => (
            "client",
            "conversation.item.truncate",
            None,
            event["audio_end_ms"].as_f64().map(|ms| ms as u64),
        ),
        (false, "input_audio_buffer.speech_started") => {
            ("cascade", "input_audio_buffer.speech_started", None, None)
        }
        (false, "input_audio_buffer.speech_stopped") => {
            ("cascade", "input_audio_buffer.speech_stopped", None, None)
        }
        (false, "conversation.item.input_audio_transcription.completed") => (
            "cascade",
            "conversation.item.input_audio_transcription.completed",
            None,
            None,
        ),
        (false, "response.created") => ("cascade", "response.created", None, None),
        (false, "response.done") => (
            "cascade",
            "response.done",
            event["response"]["status"].as_str().map(|status| {
                ["completed", "cancelled", "failed", "incomplete"]
                    .into_iter()
                    .find(|known| *known == status)
                    .unwrap_or("other")
            }),
            None,
        ),
        _ => return None,
    };
    Some(VoiceTrace {
        from,
        kind,
        status,
        audio_end_ms,
    })
}

fn log_voice_trace(conversation: Uuid, from_client: bool, event: Option<&serde_json::Value>) {
    if let Some(trace) = event.and_then(|event| voice_trace(from_client, event)) {
        tracing::info!(
            %conversation,
            from = trace.from,
            event = trace.kind,
            status = trace.status,
            audio_end_ms = trace.audio_end_ms,
            "voice event"
        );
    }
}

fn parse_event(text: &str) -> Option<serde_json::Value> {
    serde_json::from_str(text).ok()
}

fn event_kind(event: &serde_json::Value) -> Option<&str> {
    event["type"].as_str()
}

/// 兩邊互相轉送到其中一邊斷線。內容一律不寫日誌。
async fn relay(client: WebSocket, mut session: VoiceSession, state: AppState) {
    let Some(mut notices) = session.take_notices() else {
        return;
    };
    let mut heard = HeardTracker::default();
    let Some(cascade) = connect(&mut session, &state).await else {
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
    let mut revoke_check = tokio::time::interval(REVOKE_CHECK);
    loop {
        tokio::select! {
            // 節點被停用或刪除：結束這場，瀏覽器會當成中斷，學生重新按「即時」就改選別的節點
            _ = revoke_check.tick() => {
                if !state.voice_nodes.still_serving(&state, session.node()).await {
                    break;
                }
            }
            frame = from_client.next() => match frame {
                Some(Ok(ClientFrame::Text(text))) => {
                    let event = parse_event(&text);
                    log_voice_trace(session.conversation, true, event.as_ref());
                    if let Some(event) = &event {
                        heard.from_client(event);
                    }
                    let allowed = event
                        .as_ref()
                        .is_some_and(|e| event_kind(e).is_some_and(allow_client_event));
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
                    log_voice_trace(session.conversation, false, event.as_ref());
                    if let Some(chars) = event.as_ref().and_then(|e| heard.from_cascade(e)) {
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

type CascadeSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// 連上場次綁定的節點（每次最多等 `CONNECT_TIMEOUT`）。失敗就立即把該節點標為不健康，換下一個候選
/// 重試一次；仍失敗才放棄。
async fn connect(session: &mut VoiceSession, state: &AppState) -> Option<CascadeSocket> {
    let mut failed = Vec::new();
    for attempt in 0..2 {
        let node = session.node();
        let url = session.url.clone();
        let connecting = tokio_tungstenite::connect_async(url.as_str());
        match tokio::time::timeout(CONNECT_TIMEOUT, connecting).await {
            Ok(Ok((socket, _))) => return Some(socket),
            _ => {
                // 日誌只記節點識別，不含網址
                tracing::warn!(%node, "cannot reach the voice cascade");
                state.voice_nodes.mark_unhealthy(node);
                failed.push(node);
            }
        }
        if attempt == 0 {
            let candidates = state.voice_nodes.candidates(state, &failed).await;
            if !state.voice_sessions.rebind(session, &candidates) {
                break;
            }
        }
    }
    None
}

/// 驗證內部端點的 Bearer，回傳這把密鑰屬於哪個節點。環境節點的密鑰比對雜湊（時間與內容無關）；
/// 其他節點每次直接以雜湊查資料庫，所以輪換與停用立即生效，不經過快取（S-19 7.3）。
async fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<NodeId, ApiError> {
    let env_secret = state.config.cascade_llm_token.as_deref();
    // 沒有任何節點時行為等同未啟用
    let rejected = || async {
        if env_secret.is_none() && state.voice_nodes.registered(&state.pool).await.is_empty() {
            ApiError::not_found()
        } else {
            ApiError::unauthorized()
        }
    };
    let Some(given) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    else {
        return Err(rejected().await);
    };
    let digest = Sha256::digest(given.as_bytes());
    if env_secret.is_some_and(|secret| digest == Sha256::digest(secret.as_bytes())) {
        return Ok(NodeId::Env);
    }
    let node: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM voice_nodes WHERE token_hash = $1 AND status <> 'disabled'",
    )
    .bind(digest.as_slice())
    .fetch_optional(&state.pool)
    .await?;
    match node {
        Some(id) => Ok(NodeId::Db(id)),
        None => Err(rejected().await),
    }
}

async fn models(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    authenticate(&state, &headers).await?;
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
    abandoned: Abandoned,
    state: AppState,
    token: String,
) -> Response {
    let begin = (text, state, token, ReplyWatch::new(abandoned));
    let chunks = stream::unfold(begin, |(mut text, state, token, mut watch)| async move {
        if watch.finished {
            return None;
        }
        loop {
            match text.recv().await {
                Some(ReplyText::Delta(delta)) => {
                    watch.sent_any = true;
                    return Some((delta_chunk(&delta), (text, state, token, watch)));
                }
                Some(ReplyText::Failed(code)) => state.voice_sessions.turn_failed(&token, code),
                None if watch.sent_any => {
                    watch.finished = true;
                    let end = Bytes::from_static(END_OF_REPLY.as_bytes());
                    return Some((end, (text, state, token, watch)));
                }
                None => {
                    watch.sent_any = true;
                    return Some((delta_chunk(FAILED_REPLY), (text, state, token, watch)));
                }
            }
        }
    });
    sse(Body::from_stream(chunks.map(Ok::<_, Infallible>)))
}

/// 回覆串流的進度。串流被丟棄時還沒結束，代表 cascade 放棄了它（打斷或逾時）：留一行診斷日誌，
/// 分辨是一開始就被掐斷，還是說到一半。只記時間與是否已送出內容，不含任何原文。
struct ReplyWatch {
    started: Instant,
    sent_any: bool,
    finished: bool,
    abandoned: Abandoned,
}

impl ReplyWatch {
    fn new(abandoned: Abandoned) -> Self {
        Self {
            started: Instant::now(),
            sent_any: false,
            finished: false,
            abandoned,
        }
    }
}

impl Drop for ReplyWatch {
    fn drop(&mut self) {
        if !self.finished {
            tracing::info!(
                sent_any = self.sent_any,
                elapsed_ms = self.started.elapsed().as_millis() as u64,
                "voice reply stream dropped before it finished"
            );
            // 一個字都沒送出就被放棄：學生不會聽到這次的回覆，不必再等 AI，也別擋住接著說的下一句
            if !self.sent_any {
                self.abandoned.notify_one();
            }
        }
    }
}

/// cascade 的 LLM 節點：學生的一句話走與文字相同的回覆流程。
async fn chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<ChatBody>, JsonRejection>,
) -> Result<Response, ApiError> {
    let caller = authenticate(&state, &headers).await?;
    let Json(body) = body.map_err(|_| ApiError::invalid_message())?;
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
    // 一個被入侵的節點不能代操其他節點的場次
    if state.voice_sessions.node_of(&token) != Some(caller) {
        return Err(ApiError::unauthorized());
    }
    let turn = spoken_turn(&state, &token, user, id, &body.messages).await;
    if turn.is_err() {
        state.voice_sessions.turn_failed(&token, None);
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
        return Err(ApiError::invalid_message());
    }
    let user_turns = messages.iter().filter(|m| m.role == "user").count();
    let turn_lock = state
        .voice_sessions
        .turn_lock(token)
        .ok_or(ApiError::not_found())?;
    let _turn = turn_lock.lock().await;
    let fresh = state.voice_sessions.new_speech(token, content, user_turns);
    let joined = fresh != content;
    // 診斷：cascade 把學生這句話送來了（只記回合數與是否併入前一句，不含內容）
    tracing::info!(conversation = %id, user_turns, joined, "voice turn received");
    let mut attempts = 0;
    let submitted = loop {
        // 回覆沒被聽到又接著說：存成獨立的一則（畫面是兩個泡泡），但不多算一回合
        let rejoined = if joined {
            rejoin_message(state, user, id, &fresh, SOURCE).await
        } else {
            Ok(None)
        };
        let result = match rejoined {
            Ok(Some(submitted)) => Ok(submitted),
            Ok(None) => match submit_message(state, user, id, &fresh, SOURCE, true).await {
                // 最後一則是沒有回覆的學生訊息（例如回覆沒產生、瀏覽器逾時後重連，新會話沒有記憶）：
                // 不擋下，接著說的話存成獨立的一則、回合數不變，AI 一次回答兩句
                Err(e) if e.code == "unanswered_message" => {
                    match rejoin_message(state, user, id, &fresh, SOURCE).await {
                        Ok(Some(submitted)) => Ok(submitted),
                        // 這時最後一則已是聽到的 AI 回覆（交易之間有人回覆了）：照原本的錯誤處理
                        Ok(None) => Err(e),
                        Err(rejoin_error) => Err(rejoin_error),
                    }
                }
                other => other,
            },
            Err(e) => Err(e),
        };
        match result {
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
            let (text, abandoned) =
                begin_text_stream(state.clone(), user, *conversation, first).await?;
            Ok(streamed_reply(
                text,
                abandoned,
                state.clone(),
                token.to_string(),
            ))
        }
    }
}
