//! F-10 語音備援：授權、輸入驗證、錯誤邊界，以及對本機假 OpenAI 伺服器的請求格式（合成資料）。
//! 音訊只在記憶體中轉送，不保存（S-05.4）。

mod common;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

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
    config::Config,
    voice::{OpenAiVoice, SPEECH_PACING_HEADER, VoiceError, VoiceProvider},
};
use sqlx::PgPool;
use tokio::sync::Notify;

/// 假的語音服務：記錄收到的內容，並依設定成功或失敗。
#[derive(Default)]
struct FakeVoice {
    fail: bool,
    self_hosted: bool,
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

    fn self_hosted_speech(&self) -> bool {
        self.self_hosted
    }
}

/// 自架語音：合成會停住，直到測試放行，用來讓請求同時進行。
struct HeldVoice {
    entered: Notify,
    release: Notify,
}

#[async_trait]
impl VoiceProvider for HeldVoice {
    async fn transcribe(&self, _: Vec<u8>, _: &str, _: &str) -> Result<String, VoiceError> {
        Err(VoiceError::Failed)
    }

    async fn speak(&self, _: &str, _: &str) -> Result<Vec<u8>, VoiceError> {
        self.entered.notify_one();
        self.release.notified().await;
        Ok(vec![1, 2, 3])
    }

    fn self_hosted_speech(&self) -> bool {
        true
    }
}

impl HeldVoice {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: Notify::new(),
            release: Notify::new(),
        })
    }

    /// 等一個請求開始合成；請求被擋下時要失敗，而不是一直等。
    async fn wait_for_synthesis(&self) {
        tokio::time::timeout(Duration::from_secs(5), self.entered.notified())
            .await
            .expect("a request should reach the provider");
    }
}

fn app_with_voice(pool: PgPool, voice: Arc<dyn VoiceProvider>) -> Router {
    app_with_voice_config(pool, voice, test_config())
}

fn app_with_voice_config(pool: PgPool, voice: Arc<dyn VoiceProvider>, config: Config) -> Router {
    register_fixture_pool(&pool);
    let state =
        AppState::new(pool, config, Arc::new(FakeIdentity), FakeAi::with(vec![])).with_voice(voice);
    app(state)
}

fn speech_req(cookie: &str) -> Request<Body> {
    json_req(
        "POST",
        "/api/voice/speech",
        Some(cookie),
        Some(json!({"text": "你覺得呢？", "language": "zh-TW"})),
    )
}

async fn used_ai_calls(app: &Router, cookie: &str) -> u64 {
    let res = send(app, get("/api/me/usage", Some(cookie))).await;
    json_body(res).await["used"].as_u64().unwrap()
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
    // 與其他端點相同的 JSON 錯誤格式（#324）
    assert_json_error(res, StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large").await;
}

#[sqlx::test]
async fn transcribe_accepts_audio_larger_than_the_json_body_limit(pool: PgPool) {
    // 語音辨識有自己的 10 MB 上限，不受全域 JSON body 上限影響（#324）
    let voice = Arc::new(FakeVoice::default());
    let app = app_with_voice(pool, voice.clone());
    let a = seed_actors(&app).await;
    let size = socrates_chat_backend::MAX_BODY_BYTES * 3;
    assert!(size < socrates_chat_backend::voice::MAX_AUDIO_BYTES);
    let res = send(
        &app,
        audio_req(
            "/api/voice/transcribe?language=en",
            Some(&a.student),
            "audio/webm",
            vec![0; size],
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(voice.calls.lock().unwrap()[0].0, size);
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
async fn metered_speech_counts_every_request_and_asks_the_client_to_merge(pool: PgPool) {
    let app = app_with_voice(pool, Arc::new(FakeVoice::default()));
    let a = seed_actors(&app).await;
    for expected in 1..=2 {
        let res = send(&app, speech_req(&a.student)).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().get(SPEECH_PACING_HEADER).is_none());
        assert_eq!(used_ai_calls(&app, &a.student).await, expected);
    }
}

#[sqlx::test]
async fn self_hosted_speech_is_not_ai_usage_and_goes_sentence_by_sentence(pool: PgPool) {
    let voice = FakeVoice {
        self_hosted: true,
        ..Default::default()
    };
    let config = Config {
        ai_usage_max: 1,
        ai_usage_window: Duration::from_secs(3600),
        ..test_config()
    };
    let app = app_with_voice_config(pool, Arc::new(voice), config);
    let a = seed_actors(&app).await;
    // 超過額度也能唸：自架服務不是外部 AI 費用
    for _ in 0..3 {
        let res = send(&app, speech_req(&a.student)).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()[SPEECH_PACING_HEADER], "sentence");
    }
    assert_eq!(used_ai_calls(&app, &a.student).await, 0);
}

#[sqlx::test]
async fn self_hosted_speech_allows_one_request_per_user_at_a_time(pool: PgPool) {
    let voice = HeldVoice::new();
    let app = app_with_voice(pool, voice.clone());
    let a = seed_actors(&app).await;

    let running = {
        let (app, cookie) = (app.clone(), a.student.clone());
        tokio::spawn(async move { send(&app, speech_req(&cookie)).await.status() })
    };
    voice.wait_for_synthesis().await;

    let busy = send(&app, speech_req(&a.student)).await;
    assert_eq!(busy.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(json_body(busy).await["error"]["code"], "speech_busy");

    voice.release.notify_one();
    assert_eq!(running.await.unwrap(), StatusCode::OK);

    // 前一個結束後名額就釋出
    let again = {
        let (app, cookie) = (app.clone(), a.student.clone());
        tokio::spawn(async move { send(&app, speech_req(&cookie)).await.status() })
    };
    voice.wait_for_synthesis().await;
    voice.release.notify_one();
    assert_eq!(again.await.unwrap(), StatusCode::OK);
}

#[sqlx::test]
async fn self_hosted_speech_frees_the_slot_when_the_client_goes_away(pool: PgPool) {
    let voice = HeldVoice::new();
    let app = app_with_voice(pool, voice.clone());
    let a = seed_actors(&app).await;

    // 學生打斷時前端中止請求，後端的 handler 就被丟棄
    let interrupted = {
        let (app, cookie) = (app.clone(), a.student.clone());
        tokio::spawn(async move { send(&app, speech_req(&cookie)).await.status() })
    };
    voice.wait_for_synthesis().await;
    interrupted.abort();
    assert!(interrupted.await.unwrap_err().is_cancelled());

    let next = {
        let (app, cookie) = (app.clone(), a.student.clone());
        tokio::spawn(async move { send(&app, speech_req(&cookie)).await.status() })
    };
    voice.wait_for_synthesis().await;
    voice.release.notify_one();
    assert_eq!(next.await.unwrap(), StatusCode::OK);
}

#[sqlx::test]
async fn self_hosted_speech_keeps_users_apart(pool: PgPool) {
    let voice = HeldVoice::new();
    let app = app_with_voice(pool, voice.clone());
    let a = seed_actors(&app).await;
    let other = add_student(&app, &a, "other-student", "other@example.test").await;

    let running = {
        let (app, cookie) = (app.clone(), a.student.clone());
        tokio::spawn(async move { send(&app, speech_req(&cookie)).await.status() })
    };
    voice.wait_for_synthesis().await;

    let second = {
        let (app, cookie) = (app.clone(), other.clone());
        tokio::spawn(async move { send(&app, speech_req(&cookie)).await.status() })
    };
    voice.wait_for_synthesis().await;
    voice.release.notify_one();
    voice.release.notify_one();
    assert_eq!(running.await.unwrap(), StatusCode::OK);
    assert_eq!(second.await.unwrap(), StatusCode::OK);
}

#[sqlx::test]
async fn self_hosted_speech_frees_the_slot_when_synthesis_fails(pool: PgPool) {
    let voice = FakeVoice {
        self_hosted: true,
        fail: true,
        ..Default::default()
    };
    let app = app_with_voice(pool, Arc::new(voice));
    let a = seed_actors(&app).await;
    for _ in 0..2 {
        let res = send(&app, speech_req(&a.student)).await;
        assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
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
async fn openai_voice_sends_transcription_to_the_stt_base_url_when_set() {
    // 辨識可指向自架的 Whisper 服務；合成與對話仍走 OPENAI_BASE_URL
    let (chat_base, chat_calls) = spawn_fake_openai().await;
    let (stt_base, stt_calls) = spawn_fake_openai().await;
    let mut c = test_config();
    c.openai_base_url = chat_base;
    c.openai_stt_base_url = Some(stt_base);
    c.openai_stt_model = Some("stt-test".into());
    c.openai_tts_model = Some("tts-test".into());
    let v = OpenAiVoice::from_config(&c);
    v.transcribe(vec![5; 20], "audio/webm", "en").await.unwrap();
    v.speak("hi", "en").await.unwrap();
    assert_eq!(stt_calls.lock().unwrap().len(), 1);
    assert_eq!(stt_calls.lock().unwrap()[0].0, "/audio/transcriptions");
    assert_eq!(chat_calls.lock().unwrap().len(), 1);
    assert_eq!(chat_calls.lock().unwrap()[0].0, "/audio/speech");
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
async fn speech_goes_to_the_tts_base_url_without_the_openai_key_when_set() {
    let (chat_base, chat_calls) = spawn_fake_openai().await;
    let (tts_base, tts_calls) = spawn_fake_openai().await;
    let mut c = test_config();
    c.openai_base_url = chat_base;
    c.openai_tts_base_url = Some(format!("{tts_base}/"));
    c.openai_tts_model = Some("tts-test".into());
    let voice = OpenAiVoice::from_config(&c);

    assert!(voice.self_hosted_speech());
    assert_eq!(voice.speak("你好", "zh").await.unwrap(), vec![9, 8, 7]);
    assert!(chat_calls.lock().unwrap().is_empty());
    let calls = tts_calls.lock().unwrap();
    assert_eq!(calls[0].0, "/audio/speech");
    assert_eq!(
        calls[0].1, "",
        "the OpenAI key must not leave for the local service"
    );
}

#[tokio::test]
async fn self_hosted_speech_works_when_every_openai_key_is_cooling_down() {
    let (tts_base, _) = spawn_fake_openai().await;
    let mut c = test_config();
    c.openai_keys.mark_rejected(0);
    c.openai_tts_base_url = Some(tts_base);
    c.openai_tts_model = Some("tts-test".into());
    assert!(
        OpenAiVoice::from_config(&c)
            .speak("你好", "zh")
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn speech_stays_metered_without_a_tts_base_url() {
    assert!(!openai_voice("http://127.0.0.1:1").self_hosted_speech());
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

#[tokio::test]
async fn openai_voice_failures_never_change_the_shared_key_state() {
    use axum::http::StatusCode;
    for status in [StatusCode::FORBIDDEN, StatusCode::TOO_MANY_REQUESTS] {
        let app = Router::new()
            .route("/audio/transcriptions", post(move || async move { status }))
            .route("/audio/speech", post(move || async move { status }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut c = test_config();
        c.openai_base_url = base;
        c.openai_stt_model = Some("stt-test".into());
        c.openai_tts_model = Some("tts-test".into());
        let v = OpenAiVoice::from_config(&c);
        assert_eq!(
            v.transcribe(vec![1], "audio/webm", "en").await,
            Err(VoiceError::Failed)
        );
        assert_eq!(v.speak("hi", "en").await, Err(VoiceError::Failed));
        // 語音端點的錯誤（例如模型沒開放）不能讓對話用的 key 被冷卻或停用
        assert_eq!(c.openai_keys.usable_count(), 1, "{status}");
        assert!(c.openai_keys.acquire().is_some(), "{status}");
    }
}
