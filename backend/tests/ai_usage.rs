//! 每位使用者的 AI 用量上限（滑動時間窗）與 `GET /api/me/usage`。全部使用合成資料與假 AI。

mod common;

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use common::Script::*;
use common::*;
use serde_json::{Value, json};
use socrates_chat_backend::{
    AppState, app,
    config::Config,
    voice::{VoiceError, VoiceProvider},
};
use sqlx::PgPool;

const REPLY: [&str; 2] = [
    "你為什麼",
    "這樣認為？<<<META>>>{\"question_type\":\"clarify\"}",
];

fn limited(max: usize) -> Config {
    Config {
        ai_usage_max: max,
        ai_usage_window: Duration::from_secs(3600),
        ..test_config()
    }
}

async fn usage(app: &Router, cookie: Option<&str>) -> (StatusCode, Value) {
    let res = send(app, get("/api/me/usage", cookie)).await;
    (res.status(), json_body(res).await)
}

/// 送出學生訊息並讀完 SSE，回傳訊息之後的事件（delta／done／error）。
async fn say(app: &Router, a: &Actors, conv: &str, text: &str) -> Vec<(String, Value)> {
    let res = send(
        app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/messages"),
            Some(&a.student),
            Some(json!({"content": text})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let mut events = parse_sse(&text_body(res).await);
    assert_eq!(events.remove(0).0, "message");
    events
}

async fn retry_stream(app: &Router, a: &Actors, conv: &str) -> Vec<(String, Value)> {
    let res = send(
        app,
        get(
            &format!("/api/conversations/{conv}/stream"),
            Some(&a.student),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    parse_sse(&text_body(res).await)
}

fn last(events: &[(String, Value)]) -> (&str, &Value) {
    let (name, data) = events.last().unwrap();
    (name.as_str(), data)
}

#[sqlx::test]
async fn usage_requires_login(pool: PgPool) {
    let app = test_app_with_config(pool, FakeAi::with(vec![]), limited(3));
    assert_eq!(usage(&app, None).await.0, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn usage_starts_at_zero_with_the_configured_limit(pool: PgPool) {
    let app = test_app_with_config(pool, FakeAi::with(vec![]), limited(3));
    let a = seed_actors(&app).await;
    let (status, body) = usage(&app, Some(&a.student)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({"used": 0, "limit": 3, "window_secs": 3600, "resets_in_secs": null})
    );
}

#[sqlx::test]
async fn each_chat_reply_uses_one_call_and_the_limit_blocks_the_next(pool: PgPool) {
    let ai = FakeAi::with(vec![
        Reply(REPLY.to_vec()),
        Reply(REPLY.to_vec()),
        Reply(REPLY.to_vec()),
    ]);
    let app = test_app_with_config(pool, ai.clone(), limited(2));
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;

    assert_eq!(last(&say(&app, &a, &conv, "一").await).0, "done");
    assert_eq!(last(&say(&app, &a, &conv, "二").await).0, "done");
    let (_, body) = usage(&app, Some(&a.student)).await;
    assert_eq!(
        (body["used"].clone(), body["limit"].clone()),
        (json!(2), json!(2))
    );
    assert!(body["resets_in_secs"].as_u64().unwrap() > 0);

    let before = ai.request_count();
    let events = say(&app, &a, &conv, "三").await;
    assert_eq!(last(&events).0, "error");
    assert_eq!(last(&events).1["code"], "ai_usage_exceeded");
    assert_eq!(ai.request_count(), before, "超過上限時不能碰到 AI");

    // 重試也一樣被擋，且不再增加用量
    let events = retry_stream(&app, &a, &conv).await;
    assert_eq!(last(&events).1["code"], "ai_usage_exceeded");
    assert_eq!(usage(&app, Some(&a.student)).await.1["used"], 2);
}

#[sqlx::test]
async fn a_failed_ai_call_does_not_use_up_the_budget(pool: PgPool) {
    // 自動重試 1 次後仍失敗；之後手動重試成功
    let ai = FakeAi::with(vec![Fail, Fail, Reply(REPLY.to_vec())]);
    let app = test_app_with_config(pool, ai, limited(1));
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;

    let events = say(&app, &a, &conv, "一").await;
    assert_eq!(last(&events).1["code"], "ai_unavailable");
    assert_eq!(usage(&app, Some(&a.student)).await.1["used"], 0);

    assert_eq!(last(&retry_stream(&app, &a, &conv).await).0, "done");
    assert_eq!(usage(&app, Some(&a.student)).await.1["used"], 1);
}

#[sqlx::test]
async fn usage_is_counted_per_user(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(REPLY.to_vec())]);
    let app = test_app_with_config(pool, ai, limited(1));
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    assert_eq!(last(&say(&app, &a, &conv, "一").await).0, "done");

    assert_eq!(usage(&app, Some(&a.student)).await.1["used"], 1);
    assert_eq!(usage(&app, Some(&a.teacher)).await.1["used"], 0);
}

struct FakeVoice;

#[async_trait]
impl VoiceProvider for FakeVoice {
    async fn transcribe(&self, _: Vec<u8>, _: &str, _: &str) -> Result<String, VoiceError> {
        Ok("文字".into())
    }
    async fn speak(&self, _: &str, _: &str) -> Result<Vec<u8>, VoiceError> {
        Ok(vec![1, 2, 3])
    }
}

fn transcribe_req(cookie: &str) -> Request<Body> {
    Request::post("/api/voice/transcribe?language=en")
        .header(header::ORIGIN, APP_URL)
        .header(header::CONTENT_TYPE, "audio/webm")
        .header(header::COOKIE, cookie)
        .body(Body::from(vec![1u8; 10]))
        .unwrap()
}

#[sqlx::test]
async fn voice_calls_count_against_the_same_budget(pool: PgPool) {
    register_fixture_pool(&pool);
    let state = AppState::new(
        pool,
        limited(2),
        Arc::new(FakeIdentity),
        FakeAi::with(vec![]),
    )
    .with_voice(Arc::new(FakeVoice));
    let app = app(state);
    let a = seed_actors(&app).await;

    assert_eq!(
        send(&app, transcribe_req(&a.student)).await.status(),
        StatusCode::OK
    );
    let speech = json_req(
        "POST",
        "/api/voice/speech",
        Some(&a.student),
        Some(json!({"text": "hi", "language": "en"})),
    );
    assert_eq!(send(&app, speech).await.status(), StatusCode::OK);

    let res = send(&app, transcribe_req(&a.student)).await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(json_body(res).await["error"]["code"], "ai_usage_exceeded");
    assert_eq!(usage(&app, Some(&a.student)).await.1["used"], 2);
}
