//! 教室 AI 路由換 key 的日誌（S-03.1、S-09.3）：供應商限流時，`API key failed; trying the next`
//! 要帶 HTTP 狀態、Retry-After 秒數、供應商的固定錯誤代碼與剩餘額度，讓維運者分辨是哪一種限制；
//! 但不能帶 key、供應商回應本文或對話內容。全部使用合成資料。
//! 這個檔案只能有一個測試：tracing 的全域訂閱者整個程序只能設定一次。

mod common;

use std::{
    io,
    sync::{Arc, Mutex},
};

use axum::{
    Json, Router,
    http::{StatusCode, header},
    response::IntoResponse,
};
use common::*;
use serde_json::{Value, json};
use socrates_chat_backend::{
    AppState, app,
    classroom_ai_store::{self, ModelRef},
    teacher_key_crypto::KeyCipher,
};
use sqlx::PgPool;
use tracing_subscriber::fmt::MakeWriter;
use uuid::Uuid;

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

const STUDENT_MESSAGE: &str = "SECRETMESSAGE我覺得應該拉下拉桿";
const PROVIDER_BODY: &str = "SECRETPROVIDERBODY";
const TEACHER_KEY: &str = "sk-synthetic-FAILOVER-1111-2222-kkkk";
const KEY_ENCRYPTION_SECRET: &str = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";

/// 永遠回 429 的供應商：帶 Retry-After 與剩餘額度 header，本文含固定代碼與要擋掉的文字（回聲 key）。
async fn rate_limiting_provider() -> String {
    let router = Router::new().fallback(|headers: axum::http::HeaderMap| async move {
        let echoed = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        (
            StatusCode::TOO_MANY_REQUESTS,
            [
                ("retry-after", "1728"),
                ("x-ratelimit-remaining-requests", "0"),
                ("x-ratelimit-remaining-tokens", "12345"),
            ],
            Json(
                json!({"error": {"code": "rate_limit_exceeded", "type": "requests",
                "message": format!("{PROVIDER_BODY} {echoed}")}}),
            ),
        )
            .into_response()
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{addr}/v1/")
}

#[sqlx::test]
async fn failover_log_says_why_the_provider_refused_without_keys_or_bodies(pool: PgPool) {
    let logs = Captured::default();
    socrates_chat_backend::logging::json_subscriber()
        .with_writer(logs.clone())
        .with_env_filter(socrates_chat_backend::logging::env_filter(Some("trace")))
        .init();

    register_fixture_pool(&pool);
    let db = pool.clone();
    let mut config = test_config();
    config.teacher_key_cipher = Some(Arc::new(
        KeyCipher::parse(&format!("k1:{KEY_ENCRYPTION_SECRET}")).unwrap(),
    ));
    config.classroom_ai_routing = true;
    let app = app(AppState::new(
        pool,
        config,
        Arc::new(FakeIdentity),
        Arc::new(FakeAi::default()),
    )
    .with_route_endpoint(rate_limiting_provider().await));
    let a = seed_actors(&app).await;
    let added = send(
        &app,
        json_req(
            "POST",
            "/api/teacher/keys",
            Some(&a.teacher),
            Some(json!({"provider": "openai", "key": TEACHER_KEY})),
        ),
    )
    .await;
    assert!(added.status().is_success(), "{}", added.status());
    let classroom: Uuid = a.classroom.parse().unwrap();
    classroom_ai_store::set(
        &db,
        classroom,
        ModelRef {
            provider: "openai".into(),
            model: "synthetic-model".into(),
        },
        vec![],
    )
    .await
    .unwrap();
    classroom_ai_store::clear_transitional(&db, classroom)
        .await
        .unwrap();

    let conversation = start_conversation(&app, &a).await;
    let (_, events) = student_says_events(&app, &a, &conversation, STUDENT_MESSAGE).await;
    let events = events.expect("SSE stream");
    let code = events
        .iter()
        .find(|(name, _)| name == "error")
        .map(|(_, data)| data["code"].clone());
    assert_eq!(code, Some(json!("classroom_ai_unavailable")));

    let raw = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    let failover: Vec<Value> = raw
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|line| line.get("fields").cloned())
        .filter(|f| f["message"] == json!("API key failed; trying the next"))
        .collect();
    assert_eq!(failover.len(), 1, "{failover:?}");
    let f = &failover[0];
    assert_eq!(f["class"], json!("rate_limited"));
    assert_eq!(f["http_status"], json!(429));
    assert_eq!(f["retry_after_secs"], json!(1728));
    assert_eq!(f["provider_code"], json!("rate_limit_exceeded"));
    assert_eq!(f["ratelimit_remaining_requests"], json!(0));
    assert_eq!(f["ratelimit_remaining_tokens"], json!(12345));

    for (what, secret) in [
        ("教師 API key", TEACHER_KEY),
        ("供應商回應本文", PROVIDER_BODY),
        ("學生訊息", STUDENT_MESSAGE),
    ] {
        assert!(!raw.contains(secret), "日誌出現{what}");
    }
}
