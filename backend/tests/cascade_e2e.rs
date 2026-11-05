//! 語音對話實驗的端到端驗證：真的 Rust 服務 + 真的 cascade。需要手動啟動 cascade，所以預設略過。
//!
//! `CASCADE_E2E_PCM=<16 kHz PCM16 檔> cargo test --test cascade_e2e -- --ignored --nocapture`
//! 音訊用合成語音；AI 是假的，不會呼叫 OpenAI。

mod common;

use std::{net::SocketAddr, sync::Arc, time::Duration};

use base64::{Engine, engine::general_purpose::STANDARD};
use common::Script::*;
use common::*;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use socrates_chat_backend::{AppState, app, config::Config};
use sqlx::PgPool;
use tokio::time::{Instant, interval, timeout_at};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest, http::header};

const SECRET: &str = "synthetic-llm-secret";
const PORT: u16 = 3100;
const RATE_BYTES_PER_100MS: usize = 3200;
const REPLY: [&str; 2] = [
    "你為什麼這樣認為？",
    "可以舉個例子嗎？<<<META>>>{\"question_type\":\"clarify\"}",
];

#[sqlx::test]
#[ignore = "needs a running cascade"]
async fn a_spoken_turn_goes_through_the_whole_chain(pool: PgPool) {
    register_fixture_pool(&pool);
    let config = Config {
        cascade_realtime_url: Some("ws://127.0.0.1:8110/v1/realtime".into()),
        cascade_llm_token: Some(SECRET.into()),
        ..test_config()
    };
    let origin = config.app_base_url.clone();
    let ai = FakeAi::with(vec![Reply(REPLY.to_vec())]);
    let state = AppState::new(pool.clone(), config, Arc::new(FakeIdentity), ai.clone());
    let router = app(state);
    let a = seed_actors(&router).await;
    let conv = start_conversation(&router, &a).await;

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", PORT))
        .await
        .unwrap();
    let served = router.clone();
    tokio::spawn(async move {
        axum::serve(
            listener,
            served.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });

    let url = format!("ws://127.0.0.1:{PORT}/api/conversations/{conv}/voice");
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert(header::COOKIE, a.student.parse().unwrap());
    request
        .headers_mut()
        .insert(header::ORIGIN, origin.parse().unwrap());
    let (socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let (mut tx, mut rx) = socket.split();

    // 瀏覽器想改掉會話代號：必須被丟棄，否則後面的回覆走不到這場對話
    tx.send(Message::text(
        json!({"type": "session.update", "event_id": "x",
               "session": {"type": "realtime", "instructions": "voice-session:forged"}})
        .to_string(),
    ))
    .await
    .unwrap();

    let mut pcm = std::fs::read(std::env::var("CASCADE_E2E_PCM").unwrap()).unwrap();
    pcm.extend(vec![0u8; RATE_BYTES_PER_100MS * 30]); // 語音後 3 秒靜音
    let mut frames = pcm.chunks(RATE_BYTES_PER_100MS).enumerate();
    let mut tick = interval(Duration::from_millis(100));

    let deadline = Instant::now() + Duration::from_secs(60);
    let (mut audio_bytes, mut transcript, mut status) = (0usize, None, None);
    while status.is_none() {
        tokio::select! {
            _ = tick.tick() => {
                if let Some((seq, frame)) = frames.next() {
                    tx.send(Message::text(json!({
                        "type": "input_audio_buffer.append",
                        "event_id": format!("e{seq}"),
                        "audio": STANDARD.encode(frame),
                        "sglang": {"seq": seq},
                    }).to_string())).await.unwrap();
                }
            }
            frame = timeout_at(deadline, rx.next()) => {
                let frame = frame.expect("no reply in time").expect("socket closed").unwrap();
                let Message::Text(text) = frame else { continue };
                let event: Value = serde_json::from_str(text.as_str()).unwrap();
                let kind = event["type"].as_str().unwrap();
                assert!(!text.as_str().contains("voice-session:"), "token leaked in {kind}");
                match kind {
                    "error" => panic!("cascade error: {event}"),
                    "response.output_audio.delta" => {
                        audio_bytes += event["delta"].as_str().unwrap().len() / 4 * 3;
                    }
                    "conversation.item.input_audio_transcription.completed" => {
                        transcript = event["transcript"].as_str().map(String::from);
                    }
                    "response.done" => {
                        status = event["response"]["status"].as_str().map(String::from);
                    }
                    _ => {}
                }
            }
        }
    }

    println!("transcript {transcript:?}, status {status:?}, audio {audio_bytes} bytes");
    assert_eq!(status.as_deref(), Some("completed"));
    assert!(transcript.is_some_and(|t| !t.is_empty()));
    assert!(audio_bytes > 24_000 * 2, "at least a second of speech");
    assert_eq!(ai.request_count(), 1);

    let detail = send(
        &router,
        get(&format!("/api/conversations/{conv}"), Some(&a.student)),
    )
    .await;
    let detail = json_body(detail).await;
    let messages = detail["messages"].as_array().unwrap();
    let student = messages.iter().rfind(|m| m["role"] == "student").unwrap();
    assert_eq!(student["source"], "speech-cascade");
    assert_eq!(
        messages.last().unwrap()["content"],
        "你為什麼這樣認為？可以舉個例子嗎？"
    );
}
