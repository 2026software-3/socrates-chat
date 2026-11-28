//! 對話回覆每次 AI 呼叫失敗都留一行 warn：分類、HTTP 狀態、錯誤種類、失敗在哪個階段（還沒開始串流／
//! 等第一段／串流中途）與是否已送出內容給學生；不含 key、提示或對話原文。
//! 用來分辨 Gemini 是回 503 還是串流中途出錯。全部使用合成資料與假 AI。
//! 訂閱者以 `set_default` 設定，避免測試互相干擾。

mod common;

use std::{
    io,
    sync::{Arc, Mutex},
};

use common::{Script::*, *};
use serde_json::Value;
use sqlx::PgPool;
use tracing_subscriber::fmt::MakeWriter;

const STUDENT_TEXT: &str = "STUDENTMSG-SENTINEL 我覺得應該拉桿";
const REPLY_TEXT: &str = "AIREPLY-SENTINEL 為什麼？";

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

    fn failures(&self) -> Vec<Value> {
        self.text()
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter(|l| l["fields"]["message"] == "chat reply attempt failed")
            .collect()
    }
}

async fn run(pool: PgPool, scripts: Vec<Script>) -> Captured {
    let logs = Captured::default();
    let _guard = tracing::subscriber::set_default(
        socrates_chat_backend::logging::json_subscriber()
            .with_writer(logs.clone())
            .with_max_level(tracing::Level::INFO)
            .finish(),
    );
    let app = test_app_with_ai(pool, FakeAi::with(scripts));
    let actors = seed_actors(&app).await;
    let conversation = start_conversation(&app, &actors).await;
    let (status, _events) = student_says_events(&app, &actors, &conversation, STUDENT_TEXT).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    logs
}

#[sqlx::test]
async fn a_failure_before_streaming_is_logged_and_then_retried(pool: PgPool) {
    let logs = run(pool, vec![Fail, Reply(vec![REPLY_TEXT])]).await;
    let failures = logs.failures();
    assert_eq!(failures.len(), 1, "{}", logs.text());
    let f = &failures[0];
    assert_eq!(f["level"], "WARN");
    assert_eq!(f["fields"]["class"], "service_error");
    assert_eq!(f["fields"]["stage"], "request");
    assert_eq!(f["fields"]["emitted"], false);
    assert_eq!(f["fields"]["retry"], 0);
    assert!(f["fields"]["elapsed_ms"].is_u64(), "{f}");
    let text = logs.text();
    assert!(!text.contains("SENTINEL"), "{text}");
}

#[sqlx::test]
async fn a_failure_mid_stream_is_logged_with_its_stage_and_not_retried(pool: PgPool) {
    let logs = run(pool, vec![FailAfter(vec![REPLY_TEXT])]).await;
    let failures = logs.failures();
    assert_eq!(failures.len(), 1, "{}", logs.text());
    let f = &failures[0];
    assert_eq!(f["fields"]["stage"], "mid_stream");
    assert_eq!(f["fields"]["emitted"], true);
    let text = logs.text();
    assert!(!text.contains("SENTINEL"), "{text}");
}
