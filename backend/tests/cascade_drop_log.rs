//! cascade 放棄語音回覆串流時，後端留下一行診斷日誌（時間點與是否已送出內容），不含任何原文。
//! 用來分辨「回覆一開始就被掐斷」與「說到一半被打斷」；全部使用合成資料與假 AI。

mod common;

use std::{
    io,
    sync::{Arc, Mutex},
};

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use common::Script::*;
use common::*;
use serde_json::json;
use socrates_chat_backend::{AppState, app, cascade::voice_session_prompt, config::Config};
use sqlx::PgPool;
use tracing_subscriber::fmt::MakeWriter;
use uuid::Uuid;

const SECRET: &str = "synthetic-llm-secret";
const CHAT: &str = "/internal/cascade/v1/chat/completions";
const SPOKEN: &str = "SECRETSPEECH我覺得自由意志很難懂";

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

struct Sink(Arc<Mutex<Vec<u8>>>);

impl io::Write for Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Sink;
    fn make_writer(&'a self) -> Sink {
        Sink(self.0.clone())
    }
}

impl Captured {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

/// 把這個測試的日誌擷取下來；回傳的 guard 離開作用域時還原。
fn capture() -> (Captured, tracing::subscriber::DefaultGuard) {
    let logs = Captured::default();
    let guard = tracing::subscriber::set_default(
        socrates_chat_backend::logging::json_subscriber()
            .with_writer(logs.clone())
            .with_max_level(tracing::Level::INFO)
            .finish(),
    );
    (logs, guard)
}

fn enabled() -> Config {
    Config {
        cascade_realtime_url: Some("ws://127.0.0.1:1/v1/realtime".into()),
        cascade_llm_token: Some(SECRET.into()),
        ..test_config()
    }
}

/// 開一場語音並送出一輪 cascade 請求，回傳回應（尚未讀取）。
async fn spoken_turn(pool: PgPool) -> axum::response::Response {
    register_fixture_pool(&pool);
    let ai = FakeAi::with(vec![Reply(vec!["你為什麼", "這樣認為？"])]);
    let state = AppState::new(pool.clone(), enabled(), Arc::new(FakeIdentity), ai);
    let router = app(state.clone());
    let a = seed_actors(&router).await;
    let conv = start_conversation(&router, &a).await;
    let id: Uuid = conv.parse().unwrap();
    let user: Uuid = sqlx::query_scalar("SELECT user_id FROM conversations WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let session = state.voice_sessions.open(user, id).unwrap();
    let body = json!({
        "model": "rust",
        "stream": true,
        "messages": [
            {"role": "system", "content": voice_session_prompt(session.token())},
            {"role": "user", "content": SPOKEN},
        ],
    });
    let res = send(
        &router,
        Request::builder()
            .method("POST")
            .uri(CHAT)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {SECRET}"))
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    res
}

#[sqlx::test]
async fn a_reply_the_cascade_abandons_before_any_text_is_logged_without_the_speech(pool: PgPool) {
    let (logs, _guard) = capture();
    let res = spoken_turn(pool).await;
    // cascade 一收到回應就掐斷：串流一個字都沒讀過就被丟棄
    drop(res);

    let logs = logs.text();
    assert!(logs.contains("voice reply stream dropped"), "logs: {logs}");
    assert!(logs.contains("\"sent_any\":false"), "logs: {logs}");
    assert!(!logs.contains("SECRETSPEECH"), "logs: {logs}");
}

#[sqlx::test]
async fn a_reply_read_to_the_end_is_not_logged_as_dropped(pool: PgPool) {
    let (logs, _guard) = capture();
    let res = spoken_turn(pool).await;
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(
        String::from_utf8(bytes.to_vec())
            .unwrap()
            .contains("[DONE]")
    );

    let logs = logs.text();
    assert!(!logs.contains("voice reply stream dropped"), "logs: {logs}");
}
