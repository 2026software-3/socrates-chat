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

#[sqlx::test]
async fn audio_is_not_persisted_or_logged_after_success_or_failure(pool: PgPool) {
    let logs = CapturedLogs::default();
    tracing_subscriber::fmt()
        .with_writer(logs.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .init();

    const AUDIO_SENTINEL: &[u8] = b"RAW_AUDIO_SECRET_SENTINEL_7f19";
    let transcript_count_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&pool)
        .await
        .unwrap();

    let (success_base, success_requests) = spawn_fake_openai(StatusCode::OK).await;
    let success_app = app_with_openai_voice(pool.clone(), &success_base);
    let actors = seed_actors(&success_app).await;
    let success = send(&success_app, audio_request(&actors.student, AUDIO_SENTINEL)).await;
    assert_eq!(success.status(), StatusCode::OK);
    assert_eq!(json_body(success).await, json!({"text": "合成逐字稿"}));
    assert!(
        String::from_utf8_lossy(&success_requests.lock().unwrap()[0])
            .contains("RAW_AUDIO_SECRET_SENTINEL_7f19")
    );

    let (failure_base, failure_requests) =
        spawn_fake_openai(StatusCode::INTERNAL_SERVER_ERROR).await;
    let failure_app = app_with_openai_voice(pool.clone(), &failure_base);
    let failure = send(&failure_app, audio_request(&actors.student, AUDIO_SENTINEL)).await;
    assert_eq!(failure.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(json_body(failure).await["error"]["code"], "voice_failed");
    assert!(
        String::from_utf8_lossy(&failure_requests.lock().unwrap()[0])
            .contains("RAW_AUDIO_SECRET_SENTINEL_7f19")
    );

    let transcript_count_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(transcript_count_after, transcript_count_before);

    let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(
        output.contains("speech-to-text returned an error status"),
        "expected to capture OpenAiVoice's failure log; got: {output}"
    );
    assert!(!output.contains("RAW_AUDIO_SECRET_SENTINEL_7f19"));
}
