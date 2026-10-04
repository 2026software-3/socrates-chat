//! F-10 語音備援：授權、輸入驗證、錯誤邊界，以及對本機假 OpenAI 伺服器的請求格式（合成資料）。
//! 音訊只在記憶體中轉送，不保存（S-05.4）。

mod common;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
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
use socrates_chat_backend::{
    AppState, app,
    voice::{OpenAiVoice, VoiceError, VoiceProvider},
};
use sqlx::PgPool;

/// 假的語音服務：記錄收到的內容，並依設定成功或失敗。
#[derive(Default)]
struct FakeVoice {
    fail: bool,
    calls: Mutex<Vec<(usize, String, String)>>,
}

#[async_trait]
impl VoiceProvider for FakeVoice {
    async fn transcribe(
        &self,
        audio: Vec<u8>,
        mime: &str,
        language: &str,
    ) -> Result<String, VoiceError> {
        self.calls
            .lock()
            .unwrap()
            .push((audio.len(), mime.into(), language.into()));
        if self.fail {
            Err(VoiceError::Failed)
        } else {
            Ok("我會拉桿".into())
        }
    }

    async fn speak(&self, text: &str, language: &str) -> Result<Vec<u8>, VoiceError> {
        self.calls
            .lock()
            .unwrap()
            .push((text.len(), "speak".into(), language.into()));
        if self.fail {
            Err(VoiceError::Failed)
        } else {
            Ok(vec![1, 2, 3])
        }
    }
}

fn app_with_voice(pool: PgPool, voice: Arc<dyn VoiceProvider>) -> Router {
    let state = AppState::new(
        pool,
        test_config(),
        Arc::new(FakeIdentity),
        FakeAi::with(vec![]),
    )
    .with_voice(voice);
    app(state)
}

fn audio_req(path: &str, cookie: Option<&str>, mime: &str, body: Vec<u8>) -> Request<Body> {
    let mut b = Request::post(path)
        .header(header::ORIGIN, APP_URL)
        .header(header::CONTENT_TYPE, mime);
    if let Some(c) = cookie {
        b = b.header(header::COOKIE, c);
    }
    b.body(Body::from(body)).unwrap()
}

#[sqlx::test]
async fn transcribe_returns_text_and_passes_language_and_mime(pool: PgPool) {
    let voice = Arc::new(FakeVoice::default());
    let app = app_with_voice(pool, voice.clone());
    let a = seed_actors(&app).await;
    let res = send(
        &app,
        audio_req(
            "/api/voice/transcribe?language=zh-TW",
            Some(&a.student),
            "audio/webm;codecs=opus",
            vec![0; 100],
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_body(res).await, json!({"text": "我會拉桿"}));
    assert_eq!(
        voice.calls.lock().unwrap()[0],
        (100, "audio/webm;codecs=opus".into(), "zh".into())
    );
}

#[sqlx::test]
async fn voice_endpoints_require_a_member(pool: PgPool) {
    let voice = Arc::new(FakeVoice::default());
    let app = app_with_voice(pool, voice.clone());
    let a = seed_actors(&app).await;
    for (cookie, status) in [
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        (None, StatusCode::UNAUTHORIZED),
    ] {
        let res = send(
            &app,
            audio_req(
                "/api/voice/transcribe?language=en",
                cookie,
                "audio/webm",
                vec![1],
            ),
        )
        .await;
        assert_eq!(res.status(), status);
        let res = send(
            &app,
            json_req(
                "POST",
                "/api/voice/speech",
                cookie,
                Some(json!({"text": "hi", "language": "en"})),
            ),
        )
        .await;
        assert_eq!(res.status(), status);
    }
    assert!(
        voice.calls.lock().unwrap().is_empty(),
        "未授權的請求不能碰到外部服務"
    );
}

#[sqlx::test]
async fn transcribe_validates_input(pool: PgPool) {
    let app = app_with_voice(pool, Arc::new(FakeVoice::default()));
    let a = seed_actors(&app).await;
    let cases = [
        ("/api/voice/transcribe?language=fr", "audio/webm", vec![1u8]),
        ("/api/voice/transcribe?language=en", "text/plain", vec![1]),
        ("/api/voice/transcribe?language=en", "audio/webm", vec![]),
    ];
    for (path, mime, body) in cases {
        let res = send(&app, audio_req(path, Some(&a.student), mime, body)).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{path} {mime}");
    }
    let too_big = vec![0u8; socrates_chat_backend::voice::MAX_AUDIO_BYTES + 1];
    let res = send(
        &app,
        audio_req(
            "/api/voice/transcribe?language=en",
            Some(&a.student),
            "audio/webm",
            too_big,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[sqlx::test]
async fn speech_returns_mp3_and_validates_text(pool: PgPool) {
    let app = app_with_voice(pool, Arc::new(FakeVoice::default()));
    let a = seed_actors(&app).await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/voice/speech",
            Some(&a.student),
            Some(json!({"text": " 你覺得呢？ ", "language": "zh-TW"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "audio/mpeg");
    assert_eq!(text_body(res).await.len(), 3);

    for body in [
        json!({"text": "  ", "language": "en"}),
        json!({"text": "x".repeat(4001), "language": "en"}),
        json!({"text": "hi", "language": "fr"}),
    ] {
        let res = send(
            &app,
            json_req("POST", "/api/voice/speech", Some(&a.student), Some(body)),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
}

#[sqlx::test]
async fn provider_failure_is_a_bad_gateway_without_internal_details(pool: PgPool) {
    let app = app_with_voice(
        pool,
        Arc::new(FakeVoice {
            fail: true,
            ..Default::default()
        }),
    );
    let a = seed_actors(&app).await;
    let res = send(
        &app,
        audio_req(
            "/api/voice/transcribe?language=en",
            Some(&a.student),
            "audio/webm",
            vec![1],
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
    let body = json_body(res).await;
    assert_eq!(body["error"]["code"], "voice_failed");
    assert!(body["error"]["request_id"].is_string());
}

#[sqlx::test]
async fn voice_is_unavailable_until_models_are_configured(pool: PgPool) {
    // 預設設定沒有語音模型（具體模型尚未決定）
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let res = send(
        &app,
        audio_req(
            "/api/voice/transcribe?language=en",
            Some(&a.student),
            "audio/webm",
            vec![1],
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json_body(res).await["error"]["code"], "voice_unavailable");
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/voice/speech",
            Some(&a.student),
            Some(json!({"text": "hi", "language": "en"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
}

// ---- OpenAiVoice 對本機假伺服器 ----

type Captured = Arc<Mutex<Vec<(String, String, Vec<u8>)>>>;

/// 回報收到的路徑、授權標頭與本文；辨識回 JSON，合成回固定位元組。
async fn spawn_fake_openai() -> (String, Captured) {
    async fn handle(State(c): State<Captured>, req: Request<Body>) -> axum::response::Response {
        let path = req.uri().path().to_string();
        let auth = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let bytes: Bytes = axum::body::to_bytes(req.into_body(), usize::MAX)
            .await
            .unwrap();
        c.lock().unwrap().push((path.clone(), auth, bytes.to_vec()));
        if path.ends_with("/transcriptions") {
            axum::Json(json!({"text": "  辨識結果 "})).into_response()
        } else {
            vec![9u8, 8, 7].into_response()
        }
    }
    let captured: Captured = Arc::default();
    let app = Router::new()
        .route("/audio/transcriptions", post(handle))
        .route("/audio/speech", post(handle))
        .with_state(captured.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), captured)
}

fn openai_voice(base: &str) -> OpenAiVoice {
    let mut c = test_config();
    c.openai_base_url = base.to_string();
    c.openai_stt_model = Some("stt-test".into());
    c.openai_tts_model = Some("tts-test".into());
    OpenAiVoice::from_config(&c)
}

#[tokio::test]
async fn openai_voice_sends_multipart_transcription_with_model_and_language() {
    let (base, captured) = spawn_fake_openai().await;
    let text = openai_voice(&base)
        .transcribe(vec![5; 20], "audio/mp4", "es")
        .await
        .unwrap();
    assert_eq!(text, "辨識結果");
    let calls = captured.lock().unwrap();
    let (path, auth, body) = &calls[0];
    assert_eq!(path, "/audio/transcriptions");
    assert_eq!(auth, "Bearer test-key");
    let body = String::from_utf8_lossy(body);
    for needle in [
        "name=\"model\"",
        "stt-test",
        "name=\"language\"",
        "filename=\"speech.m4a\"",
    ] {
        assert!(body.contains(needle), "missing {needle}");
    }
}

#[tokio::test]
async fn openai_voice_requests_mp3_speech_with_the_configured_model() {
    let (base, captured) = spawn_fake_openai().await;
    let audio = openai_voice(&base).speak("你好", "zh").await.unwrap();
    assert_eq!(audio, vec![9, 8, 7]);
    let calls = captured.lock().unwrap();
    assert_eq!(calls[0].0, "/audio/speech");
    let sent: serde_json::Value = serde_json::from_slice(&calls[0].2).unwrap();
    assert_eq!(sent["model"], "tts-test");
    assert_eq!(sent["input"], "你好");
    assert_eq!(sent["response_format"], "mp3");
}

#[tokio::test]
async fn openai_voice_reports_failure_when_the_service_is_unreachable() {
    // 預設假位址 127.0.0.1:1 連不上
    let v = openai_voice("http://127.0.0.1:1");
    assert_eq!(
        v.transcribe(vec![1], "audio/webm", "en").await,
        Err(VoiceError::Failed)
    );
    assert_eq!(v.speak("hi", "en").await, Err(VoiceError::Failed));
}
