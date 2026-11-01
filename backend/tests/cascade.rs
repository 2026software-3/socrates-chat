//! 語音對話實驗（cascade）：內部 LLM 端點與 WebSocket 入口的授權、用量與存檔。
//! 全部使用合成資料與假 AI，不連真的 cascade。

mod common;

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
    cascade::{allow_client_event, allow_server_event, voice_session_prompt},
    config::Config,
};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

const SECRET: &str = "synthetic-llm-secret";
const CHAT: &str = "/internal/cascade/v1/chat/completions";
const REPLY: [&str; 2] = [
    "你為什麼",
    "這樣認為？<<<META>>>{\"question_type\":\"clarify\"}",
];

fn enabled() -> Config {
    Config {
        cascade_realtime_url: Some("ws://127.0.0.1:1/v1/realtime".into()),
        cascade_llm_token: Some(SECRET.into()),
        ..test_config()
    }
}

struct Rig {
    app: Router,
    state: AppState,
    ai: Arc<FakeAi>,
}

fn rig(pool: PgPool, scripts: Vec<Script>, config: Config) -> Rig {
    register_fixture_pool(&pool);
    let ai = FakeAi::with(scripts);
    let state = AppState::new(pool, config, Arc::new(FakeIdentity), ai.clone());
    Rig {
        app: app(state.clone()),
        state,
        ai,
    }
}

async fn owner(pool: &PgPool, conversation: &str) -> Uuid {
    let id: Uuid = conversation.parse().unwrap();
    sqlx::query_scalar("SELECT user_id FROM conversations WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn llm_req(token: Option<&str>, body: Value) -> Request<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri(CHAT)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    b.body(Body::from(body.to_string())).unwrap()
}

fn turn(system: &str, text: &str) -> Value {
    json!({
        "model": "rust",
        "stream": true,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": text},
        ],
    })
}

async fn text_of(res: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// OpenAI 串流回應裡的文字。
fn deltas(body: &str) -> String {
    body.lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .filter(|d| *d != "[DONE]")
        .filter_map(|d| serde_json::from_str::<Value>(d).ok())
        .filter_map(|v| {
            v["choices"][0]["delta"]["content"]
                .as_str()
                .map(String::from)
        })
        .collect()
}

#[sqlx::test]
async fn the_llm_endpoint_needs_the_shared_secret(pool: PgPool) {
    let r = rig(pool, vec![], enabled());
    let a = seed_actors(&r.app).await;
    let body = turn("x", "y");

    let none = send(&r.app, llm_req(None, body.clone())).await;
    assert_eq!(none.status(), StatusCode::UNAUTHORIZED);
    let wrong = send(&r.app, llm_req(Some("nope"), body.clone())).await;
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);

    // 登入 cookie 不能取代共用密鑰
    let mut with_cookie = llm_req(None, body);
    with_cookie
        .headers_mut()
        .insert(header::COOKIE, a.student.parse().unwrap());
    let res = send(&r.app, with_cookie).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn the_llm_endpoint_is_absent_until_configured(pool: PgPool) {
    let r = rig(pool, vec![], test_config());
    let res = send(&r.app, llm_req(Some(SECRET), turn("x", "y"))).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn the_startup_probe_gets_a_token_without_touching_ai(pool: PgPool) {
    let r = rig(pool, vec![], enabled());
    let models = Request::builder()
        .uri("/internal/cascade/v1/models")
        .header(header::AUTHORIZATION, format!("Bearer {SECRET}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&r.app, models).await.status(), StatusCode::OK);

    let res = send(
        &r.app,
        llm_req(
            Some(SECRET),
            json!({"model": "rust", "stream": true, "max_tokens": 1,
                   "messages": [{"role": "user", "content": "hi"}]}),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(!deltas(&text_of(res).await).is_empty());
    assert_eq!(r.ai.request_count(), 0);
}

#[sqlx::test]
async fn an_unknown_voice_session_is_refused(pool: PgPool) {
    let r = rig(pool, vec![], enabled());
    let res = send(
        &r.app,
        llm_req(Some(SECRET), turn(&voice_session_prompt("forged"), "你好")),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(r.ai.request_count(), 0);
}

#[sqlx::test]
async fn a_spoken_turn_runs_the_normal_reply_pipeline(pool: PgPool) {
    let r = rig(pool.clone(), vec![Reply(REPLY.to_vec())], enabled());
    let a = seed_actors(&r.app).await;
    let conv = start_conversation(&r.app, &a).await;
    let id: Uuid = conv.parse().unwrap();
    let session = r
        .state
        .voice_sessions
        .open(owner(&pool, &conv).await, id)
        .unwrap();

    // cascade 打斷後會在 system 後面加註解，代號要仍然認得
    let system = format!(
        "{} A reply was interrupted.",
        voice_session_prompt(session.token())
    );
    let res = send(
        &r.app,
        llm_req(
            Some(SECRET),
            turn(&system, "我的信箱是 student@example.test"),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = text_of(res).await;
    assert_eq!(deltas(&body), "你為什麼這樣認為？");
    assert!(body.trim_end().ends_with("data: [DONE]"));

    // 走同一條管線：OpenAI 拿到的是遮蔽後的文字與專案的 system prompt，不是 cascade 的
    let request = r.ai.requests.lock().unwrap().last().cloned().unwrap();
    assert!(!request.system.contains("voice-session"));
    assert!(
        !request
            .messages
            .iter()
            .any(|m| m.content.contains("student@example.test"))
    );

    let detail = send(
        &r.app,
        get(&format!("/api/conversations/{conv}"), Some(&a.student)),
    )
    .await;
    let detail = json_body(detail).await;
    let messages = detail["messages"].as_array().unwrap();
    let student = messages.iter().rfind(|m| m["role"] == "student").unwrap();
    assert_eq!(student["source"], "speech-cascade");
    assert_eq!(messages.last().unwrap()["role"], "ai");

    let usage = json_body(send(&r.app, get("/api/me/usage", Some(&a.student))).await).await;
    assert_eq!(usage["used"], 1);
}

#[sqlx::test]
async fn a_user_has_one_voice_session_and_closing_frees_it(pool: PgPool) {
    let r = rig(pool, vec![], enabled());
    let user = Uuid::new_v4();
    let first = r.state.voice_sessions.open(user, Uuid::new_v4()).unwrap();
    assert!(r.state.voice_sessions.open(user, Uuid::new_v4()).is_none());
    let token = first.token().to_string();
    drop(first);
    assert!(r.state.voice_sessions.lookup(&token).is_none());
    assert!(r.state.voice_sessions.open(user, Uuid::new_v4()).is_some());
}

#[sqlx::test]
async fn the_voice_socket_checks_login_and_ownership(pool: PgPool) {
    let r = rig(pool, vec![], enabled());
    let a = seed_actors(&r.app).await;
    let conv = start_conversation(&r.app, &a).await;
    let other = add_student(&r.app, &a, "other-student", "other@example.test").await;
    let path = format!("/api/conversations/{conv}/voice");

    assert_eq!(
        send(&r.app, get(&path, None)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&r.app, get(&path, Some(&other))).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(&r.app, get(&path, Some(&a.teacher))).await.status(),
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test]
async fn the_voice_socket_refuses_other_sites(pool: PgPool) {
    let r = rig(pool, vec![], enabled());
    let a = seed_actors(&r.app).await;
    let conv = start_conversation(&r.app, &a).await;
    let req = Request::builder()
        .uri(format!("/api/conversations/{conv}/voice"))
        .header(header::COOKIE, a.student.as_str())
        .header(header::ORIGIN, "https://evil.example")
        .body(Body::empty())
        .unwrap();
    let res = send(&r.app, req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(res).await["error"]["code"], "csrf");
}

#[sqlx::test]
async fn the_voice_socket_is_unavailable_until_configured(pool: PgPool) {
    let r = rig(pool, vec![], test_config());
    let a = seed_actors(&r.app).await;
    let conv = start_conversation(&r.app, &a).await;
    let res = send(
        &r.app,
        get(
            &format!("/api/conversations/{conv}/voice"),
            Some(&a.student),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
}

async fn voice_realtime_flag(pool: PgPool, config: Config) -> Value {
    let r = rig(pool, vec![], config);
    let a = seed_actors(&r.app).await;
    let conv = start_conversation(&r.app, &a).await;
    let res = send(
        &r.app,
        get(&format!("/api/conversations/{conv}"), Some(&a.student)),
    )
    .await;
    json_body(res).await["voice_realtime"].clone()
}

#[sqlx::test]
async fn the_conversation_says_when_server_voice_is_on(pool: PgPool) {
    assert_eq!(voice_realtime_flag(pool, enabled()).await, true);
}

#[sqlx::test]
async fn the_conversation_says_when_server_voice_is_off(pool: PgPool) {
    assert_eq!(voice_realtime_flag(pool, test_config()).await, false);
}

#[test]
fn the_browser_cannot_change_the_session_or_reach_other_controls() {
    assert!(allow_client_event("input_audio_buffer.append"));
    assert!(allow_client_event("response.cancel"));
    assert!(allow_client_event("conversation.item.truncate"));
    // session.update 能改掉 instructions，也就是別人的會話代號
    assert!(!allow_client_event("session.update"));
    assert!(!allow_client_event("input_audio_buffer.commit"));
    assert!(!allow_client_event("anything.else"));
}

#[test]
fn the_session_token_never_reaches_the_browser() {
    assert!(!allow_server_event("session.created"));
    assert!(!allow_server_event("session.updated"));
    assert!(allow_server_event("session.closed"));
    assert!(allow_server_event("response.output_audio.delta"));
}
