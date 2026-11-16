//! 語音對話的診斷日誌：瀏覽器與 cascade 之間的控制事件（開始說話、停止說話、回覆建立與結束、取消、截斷）
//! 各留一行，只含事件種類、狀態與播放毫秒數，不含音訊、逐字稿或回覆文字。
//! 用來分辨「回覆被取消」是 VAD 聽到聲音、瀏覽器主動取消，還是 cascade 自己結束；全部使用合成資料。

mod common;

use std::{
    io,
    net::SocketAddr,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use common::*;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use socrates_chat_backend::{
    AppState, app,
    cascade::{VoiceTrace, voice_trace},
    config::Config,
};
use sqlx::PgPool;
use tokio::time::{sleep, timeout};
use tokio_tungstenite::tungstenite::{
    Message, client::IntoClientRequest, http::header as ws_header,
};
use tracing_subscriber::fmt::MakeWriter;

fn trace(from_client: bool, event: serde_json::Value) -> Option<VoiceTrace> {
    voice_trace(from_client, &event)
}

fn cascade_trace(kind: &'static str, status: Option<&'static str>) -> VoiceTrace {
    VoiceTrace {
        from: "cascade",
        kind,
        status,
        audio_end_ms: None,
    }
}

#[test]
fn speech_start_and_stop_from_the_cascade_are_traced() {
    assert_eq!(
        trace(false, json!({"type": "input_audio_buffer.speech_started"})),
        Some(cascade_trace("input_audio_buffer.speech_started", None))
    );
    assert_eq!(
        trace(false, json!({"type": "input_audio_buffer.speech_stopped"})),
        Some(cascade_trace("input_audio_buffer.speech_stopped", None))
    );
}

#[test]
fn response_lifecycle_is_traced_with_a_known_status_only() {
    assert_eq!(
        trace(
            false,
            json!({"type": "response.created", "response": {"id": "r1"}})
        ),
        Some(cascade_trace("response.created", None))
    );
    for status in ["completed", "cancelled", "failed", "incomplete"] {
        let event = json!({"type": "response.done", "response": {"id": "r1", "status": status}});
        assert_eq!(
            trace(false, event).and_then(|t| t.status),
            Some(status),
            "{status}"
        );
    }
    // 不認得的狀態一律寫成 other，不把對方給的字串原樣寫進日誌
    let odd = json!({"type": "response.done", "response": {"status": "SECRET-free-text"}});
    assert_eq!(trace(false, odd).and_then(|t| t.status), Some("other"));
    let none = json!({"type": "response.done", "response": {"id": "r1"}});
    assert_eq!(trace(false, none).and_then(|t| t.status), None);
}

#[test]
fn a_finished_transcription_is_traced_without_the_transcript() {
    let event = json!({
        "type": "conversation.item.input_audio_transcription.completed",
        "transcript": "SECRET我覺得自由意志很難懂",
    });
    let traced = trace(false, event).expect("traced");
    assert_eq!(
        traced.kind,
        "conversation.item.input_audio_transcription.completed"
    );
    assert!(!format!("{traced:?}").contains("SECRET"));
}

#[test]
fn browser_cancel_and_truncate_are_traced_with_the_played_milliseconds() {
    assert_eq!(
        trace(true, json!({"type": "response.cancel"})),
        Some(VoiceTrace {
            from: "client",
            kind: "response.cancel",
            status: None,
            audio_end_ms: None
        })
    );
    let truncate = json!({
        "type": "conversation.item.truncate",
        "item_id": "SECRET-item",
        "audio_end_ms": 2969.3333333333376,
    });
    assert_eq!(
        trace(true, truncate),
        Some(VoiceTrace {
            from: "client",
            kind: "conversation.item.truncate",
            status: None,
            audio_end_ms: Some(2969)
        })
    );
}

#[test]
fn audio_text_and_unknown_events_are_not_traced() {
    for (from_client, kind) in [
        (true, "input_audio_buffer.append"),
        (false, "response.output_audio.delta"),
        (false, "response.output_text.delta"),
        (false, "session.created"),
        (false, "something.unknown"),
        // 方向不對的事件不記：瀏覽器不會送 speech_started，那不是它的事件
        (true, "input_audio_buffer.speech_started"),
        (false, "response.cancel"),
    ] {
        let event = json!({"type": kind, "audio": "SECRETAUDIO", "delta": "SECRETREPLY"});
        assert_eq!(trace(from_client, event), None, "{kind}");
    }
    assert_eq!(trace(false, json!({"no": "type"})), None);
}

// ------------------------------------------------------- 經 relay 的整合測試

#[derive(Clone, Default)]
struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

struct LogSink(Arc<Mutex<Vec<u8>>>);

impl io::Write for LogSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for CapturedLogs {
    type Writer = LogSink;

    fn make_writer(&'a self) -> Self::Writer {
        LogSink(self.0.clone())
    }
}

fn logs() -> &'static CapturedLogs {
    static LOGS: OnceLock<CapturedLogs> = OnceLock::new();
    LOGS.get_or_init(|| {
        let logs = CapturedLogs::default();
        tracing::subscriber::set_global_default(
            socrates_chat_backend::logging::json_subscriber()
                .with_writer(logs.clone())
                .with_max_level(tracing::Level::INFO)
                .finish(),
        )
        .expect("no other global tracing subscriber in this test binary");
        logs
    })
}

fn log_text() -> String {
    String::from_utf8_lossy(&logs().0.lock().unwrap_or_else(|e| e.into_inner())).into_owned()
}

/// 假的 cascade：收到第一段音訊後，依序送出一輪「聽到聲音 → 辨識 → 回覆被取消」的事件，內含機密字串。
async fn spawn_scripted_cascade() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
        let mut scripted = false;
        while let Some(Ok(frame)) = socket.next().await {
            let Message::Text(text) = frame else { continue };
            if scripted || !text.as_str().contains("input_audio_buffer.append") {
                continue;
            }
            scripted = true;
            for event in [
                json!({"type": "input_audio_buffer.speech_started"}),
                json!({"type": "input_audio_buffer.speech_stopped"}),
                json!({"type": "conversation.item.input_audio_transcription.completed",
                       "transcript": "SECRETTRANSCRIPT我覺得自由意志很難懂"}),
                json!({"type": "response.created", "response": {"id": "r1"}}),
                json!({"type": "response.output_text.delta", "response_id": "r1",
                       "delta": "SECRETREPLY你為什麼這樣認為"}),
                json!({"type": "response.done", "response": {"id": "r1", "status": "cancelled"}}),
            ] {
                socket.send(Message::text(event.to_string())).await.unwrap();
            }
        }
    });
    format!("ws://{addr}/v1/realtime")
}

#[sqlx::test]
async fn the_relay_logs_control_events_in_order_and_never_their_content(pool: PgPool) {
    logs();
    register_fixture_pool(&pool);
    let config = Config {
        cascade_realtime_url: Some(spawn_scripted_cascade().await),
        cascade_llm_token: Some("synthetic-llm-secret".into()),
        ..test_config()
    };
    let origin = config.app_base_url.clone();
    let router = app(AppState::new(
        pool.clone(),
        config,
        Arc::new(FakeIdentity),
        FakeAi::with(vec![]),
    ));
    let a = seed_actors(&router).await;
    let conv = start_conversation(&router, &a).await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let served = router.clone();
    tokio::spawn(async move {
        axum::serve(
            listener,
            served.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let mut request = format!("ws://{addr}/api/conversations/{conv}/voice")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert(ws_header::COOKIE, a.student.parse().unwrap());
    request
        .headers_mut()
        .insert(ws_header::ORIGIN, origin.parse().unwrap());
    let (socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let (mut tx, mut rx) = socket.split();

    for event in [
        json!({"type": "input_audio_buffer.append", "audio": STANDARD.encode("SECRETAUDIO")}),
        json!({"type": "response.cancel"}),
        json!({"type": "conversation.item.truncate", "item_id": "SECRETITEM",
               "content_index": 0, "audio_end_ms": 2969.4}),
    ] {
        tx.send(Message::text(event.to_string())).await.unwrap();
    }

    // 等到最後一個伺服器事件（response.done）被記下來
    timeout(Duration::from_secs(10), async {
        while !(log_text().contains("response.done")
            && log_text().contains("conversation.item.truncate"))
        {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the control events should be logged");
    tx.send(Message::Close(None)).await.unwrap();
    while let Some(frame) = rx.next().await {
        if matches!(frame, Ok(Message::Close(_)) | Err(_)) {
            break;
        }
    }

    let log = log_text();
    let positions: Vec<usize> = [
        "input_audio_buffer.speech_started",
        "input_audio_buffer.speech_stopped",
        "conversation.item.input_audio_transcription.completed",
        "response.created",
        "response.done",
    ]
    .iter()
    .map(|kind| {
        log.find(kind)
            .unwrap_or_else(|| panic!("{kind} not logged: {log}"))
    })
    .collect();
    assert!(
        positions.windows(2).all(|w| w[0] < w[1]),
        "events logged out of order"
    );
    assert!(log.contains("\"status\":\"cancelled\""), "{log}");
    assert!(log.contains("response.cancel"), "{log}");
    assert!(log.contains("\"audio_end_ms\":2969"), "{log}");
    assert!(
        log.contains(&conv.to_string()),
        "the conversation id correlates the lines"
    );
    for secret in [
        "SECRETAUDIO",
        &STANDARD.encode("SECRETAUDIO"),
        "SECRETTRANSCRIPT",
        "SECRETREPLY",
        "SECRETITEM",
    ] {
        assert!(!log.contains(secret), "the log must not contain {secret}");
    }
}
