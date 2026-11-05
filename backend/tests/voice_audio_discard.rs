//! S-05.4：辨識成功或失敗後，原始音訊不得留在資料庫或日誌。

mod common;

use std::{
    io,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{Request, StatusCode, header},
    response::IntoResponse,
    routing::post,
};
use common::*;
use serde_json::json;
use socrates_chat_backend::{AppState, app, voice::OpenAiVoice};
use sqlx::PgPool;
use tracing_subscriber::fmt::MakeWriter;

const AUDIO_SENTINEL: &[u8] = b"RAW_AUDIO_SECRET_SENTINEL_7f19";

#[derive(Clone, Default)]
struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

struct LogSink(Arc<Mutex<Vec<u8>>>);

impl io::Write for LogSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
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

type CapturedRequests = Arc<Mutex<Vec<Vec<u8>>>>;

async fn spawn_fake_openai(status: StatusCode) -> (String, CapturedRequests) {
    async fn handle(
        State((status, captured)): State<(StatusCode, CapturedRequests)>,
        req: Request<Body>,
    ) -> axum::response::Response {
        let bytes: Bytes = axum::body::to_bytes(req.into_body(), usize::MAX)
            .await
            .unwrap();
        captured.lock().unwrap().push(bytes.to_vec());
        if status.is_success() {
            (status, axum::Json(json!({"text": "合成逐字稿"}))).into_response()
        } else {
            (status, "synthetic provider failure").into_response()
        }
    }

    let captured = CapturedRequests::default();
    let app = Router::new()
        .route("/audio/transcriptions", post(handle))
        .with_state((status, captured.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), captured)
}

fn app_with_openai_voice(pool: PgPool, base_url: &str) -> Router {
    let mut config = test_config();
    config.openai_base_url = base_url.to_string();
    config.openai_stt_model = Some("stt-test".into());
    let voice = Arc::new(OpenAiVoice::from_config(&config));
    register_fixture_pool(&pool);
    app(AppState::new(pool, config, Arc::new(FakeIdentity), FakeAi::with(vec![])).with_voice(voice))
}

fn audio_request(cookie: &str, audio: &[u8]) -> Request<Body> {
    Request::post("/api/voice/transcribe?language=zh-TW")
        .header(header::ORIGIN, APP_URL)
        .header(header::CONTENT_TYPE, "audio/webm")
        .header(header::COOKIE, cookie)
        .body(Body::from(audio.to_vec()))
        .unwrap()
}

/// 掃描 public schema 的每個資料表（整列轉成文字），確認原始音訊沒有出現在任何地方。
async fn assert_audio_absent_from_every_table(pool: &PgPool) {
    let hex_sentinel: String = AUDIO_SENTINEL.iter().map(|b| format!("{b:02x}")).collect();
    let plain_sentinel = String::from_utf8_lossy(AUDIO_SENTINEL);
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables
         WHERE table_schema = 'public' AND table_type = 'BASE TABLE'",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    assert!(!tables.is_empty(), "expected tables to scan");
    let mut scanned_rows = 0_usize;
    for table in tables {
        let rows: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT t::text FROM \"{table}\" t"
        )))
        .fetch_all(pool)
        .await
        .unwrap();
        scanned_rows += rows.len();
        for row in rows {
            assert!(
                !row.contains(plain_sentinel.as_ref()) && !row.contains(&hex_sentinel),
                "raw audio found in table {table}"
            );
        }
    }
    assert!(scanned_rows > 0, "expected seeded rows to scan");
}

#[sqlx::test]
async fn audio_is_not_persisted_or_logged_after_success_or_failure(pool: PgPool) {
    let logs = CapturedLogs::default();
    tracing_subscriber::fmt()
        .with_writer(logs.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .init();

    let (success_base, success_requests) = spawn_fake_openai(StatusCode::OK).await;
    let success_app = app_with_openai_voice(pool.clone(), &success_base);
    let actors = seed_actors(&success_app).await;
    let success = send(&success_app, audio_request(&actors.student, AUDIO_SENTINEL)).await;
    assert_eq!(success.status(), StatusCode::OK);
    assert_eq!(json_body(success).await, json!({"text": "合成逐字稿"}));
    assert!(
        String::from_utf8_lossy(&success_requests.lock().unwrap()[0])
            .contains(&*String::from_utf8_lossy(AUDIO_SENTINEL))
    );

    let (failure_base, failure_requests) =
        spawn_fake_openai(StatusCode::INTERNAL_SERVER_ERROR).await;
    let failure_app = app_with_openai_voice(pool.clone(), &failure_base);
    let failure = send(&failure_app, audio_request(&actors.student, AUDIO_SENTINEL)).await;
    assert_eq!(failure.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(json_body(failure).await["error"]["code"], "voice_failed");
    assert!(
        String::from_utf8_lossy(&failure_requests.lock().unwrap()[0])
            .contains(&*String::from_utf8_lossy(AUDIO_SENTINEL))
    );

    assert_audio_absent_from_every_table(&pool).await;

    let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(
        output.contains("speech-to-text returned an error status"),
        "expected to capture OpenAiVoice's failure log; got: {output}"
    );
    assert!(!output.contains(&*String::from_utf8_lossy(AUDIO_SENTINEL)));
}
