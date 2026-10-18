//! S-05.4：辨識成功或失敗後，原始音訊不得留在資料庫或日誌。

mod common;

use std::{
    io,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use common::*;
use serde_json::json;
use socrates_chat_backend::{
    AppState, app,
    voice::{VoiceError, VoiceProvider},
};
use sqlx::PgPool;
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

struct Sink(Arc<Mutex<Vec<u8>>>);

impl io::Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Sink;

    fn make_writer(&'a self) -> Self::Writer {
        Sink(self.0.clone())
    }
}

struct FakeVoice {
    fail: bool,
    received_audio: Arc<Mutex<Vec<Vec<u8>>>>,
}

#[async_trait]
impl VoiceProvider for FakeVoice {
    async fn transcribe(
        &self,
        audio: Vec<u8>,
        _mime: &str,
        _language: &str,
    ) -> Result<String, VoiceError> {
        self.received_audio.lock().unwrap().push(audio);
        if self.fail {
            Err(VoiceError::Failed)
        } else {
            Ok("合成逐字稿".into())
        }
    }

    async fn speak(&self, _text: &str, _language: &str) -> Result<Vec<u8>, VoiceError> {
        unreachable!("this test only exercises transcription")
    }
}

fn app_with_voice(pool: PgPool, voice: Arc<dyn VoiceProvider>) -> Router {
    app(AppState::new(
        pool,
        test_config(),
        Arc::new(FakeIdentity),
        FakeAi::with(vec![]),
    )
    .with_voice(voice))
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
    let logs = Captured::default();
    tracing_subscriber::fmt()
        .with_writer(logs.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .init();

    tracing::info!("audio discard log capture probe");

    const AUDIO_SENTINEL: &[u8] = b"RAW_AUDIO_SECRET_SENTINEL_7f19";
    let pool = pool;
    let transcript_count_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&pool)
        .await
        .unwrap();

    let success_audio = Arc::new(Mutex::new(Vec::new()));
    let success_app = app_with_voice(
        pool.clone(),
        Arc::new(FakeVoice {
            fail: false,
            received_audio: success_audio.clone(),
        }),
    );
    let actors = seed_actors(&success_app).await;
    let success = send(&success_app, audio_request(&actors.student, AUDIO_SENTINEL)).await;
    assert_eq!(success.status(), StatusCode::OK);
    assert_eq!(json_body(success).await, json!({"text": "合成逐字稿"}));
    assert_eq!(success_audio.lock().unwrap().as_slice(), &[AUDIO_SENTINEL]);

    let failure_audio = Arc::new(Mutex::new(Vec::new()));
    let failure_app = app_with_voice(
        pool.clone(),
        Arc::new(FakeVoice {
            fail: true,
            received_audio: failure_audio.clone(),
        }),
    );
    let failure = send(&failure_app, audio_request(&actors.student, AUDIO_SENTINEL)).await;
    assert_eq!(failure.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(json_body(failure).await["error"]["code"], "voice_failed");
    assert_eq!(failure_audio.lock().unwrap().as_slice(), &[AUDIO_SENTINEL]);

    let transcript_count_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(transcript_count_after, transcript_count_before);

    let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(output.contains("audio discard log capture probe"));
    assert!(!output.contains("RAW_AUDIO_SECRET_SENTINEL_7f19"));
}
