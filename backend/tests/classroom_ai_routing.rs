//! F-40.5 (#599): 教室對話呼叫依教室設定，用教室建立者的同供應商 key；不退回系統 key；所有資料皆為合成。
mod common;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, Response, StatusCode, header},
};
use common::*;
use serde_json::{Value, json};
use socrates_chat_backend::{
    AppState, app,
    classroom_ai_store::{self, ModelRef},
    teacher_key_crypto::KeyCipher,
    teacher_key_store::{self as keys, KeyStatus},
    teacher_key_usage::{self as usage, Range},
};
use sqlx::PgPool;
use uuid::Uuid;

const ENC: &str = "k1:MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";
const KEY_1: &str = "sk-synthetic-AAAA-1111-2222-3333-aaaa";
const KEY_2: &str = "sk-synthetic-BBBB-1111-2222-3333-bbbb";
const OTHER_TEACHER_KEY: &str = "sk-synthetic-ZZZZ-1111-2222-3333-zzzz";
const MODEL: &str = "classroom-model";

#[derive(Clone)]
enum Behavior {
    Ok,
    Status(u16, &'static str),
}

#[derive(Clone, Debug)]
struct Seen {
    key: String,
    model: String,
}

#[derive(Clone, Default)]
struct Fake {
    behaviors: Arc<Mutex<HashMap<String, Behavior>>>,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Fake {
    fn set(&self, key: &str, b: Behavior) {
        self.behaviors.lock().unwrap().insert(key.into(), b);
    }
    fn keys_seen(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.key.clone())
            .collect()
    }
}

fn sse(data: Value) -> String {
    format!("data: {data}\n\n")
}

async fn handle(State(f): State<Fake>, headers: HeaderMap, body: Bytes) -> Response<Body> {
    let key = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("")
        .to_owned();
    let parsed: Value = serde_json::from_slice(&body).unwrap_or_default();
    f.seen.lock().unwrap().push(Seen {
        key: key.clone(),
        model: parsed["model"].as_str().unwrap_or("").to_owned(),
    });
    match f
        .behaviors
        .lock()
        .unwrap()
        .get(&key)
        .cloned()
        .unwrap_or(Behavior::Ok)
    {
        Behavior::Status(status, body) => Response::builder()
            .status(StatusCode::from_u16(status).unwrap())
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap(),
        Behavior::Ok => {
            let mut b = String::new();
            for t in ["好的，", "請說說你的理由？"] {
                b += &sse(
                    json!({"id":"c","object":"chat.completion.chunk","model":MODEL,
                    "choices":[{"index":0,"delta":{"role":"assistant","content":t}}]}),
                );
            }
            b += &sse(
                json!({"id":"c","object":"chat.completion.chunk","model":MODEL,
                "choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
            );
            b += &sse(
                json!({"id":"c","object":"chat.completion.chunk","model":MODEL,"choices":[],
                "usage":{"prompt_tokens":11,"completion_tokens":7,"total_tokens":18}}),
            );
            b += "data: [DONE]\n\n";
            Response::builder()
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from(b))
                .unwrap()
        }
    }
}

async fn spawn_fake() -> (String, Fake) {
    let fake = Fake::default();
    let app = Router::new().fallback(handle).with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/v1/"), fake)
}

struct World {
    app: Router,
    a: Actors,
    pool: PgPool,
    fake: Fake,
    system: Arc<FakeAi>,
    owner: Uuid,
    classroom: Uuid,
    conv: String,
}

async fn world(pool: PgPool) -> World {
    let (endpoint, fake) = spawn_fake().await;
    register_fixture_pool(&pool);
    let mut config = test_config();
    config.teacher_key_cipher = Some(Arc::new(KeyCipher::parse(ENC).unwrap()));
    config.classroom_ai_routing = true;
    // 系統 key 路徑：若被呼叫就會回覆「SYSTEM」，測試用它確認有沒有退回系統 key
    let system = FakeAi::with((0..10).map(|_| Script::Reply(vec!["SYSTEM"])).collect());
    let app = app(
        AppState::new(pool.clone(), config, Arc::new(FakeIdentity), system.clone())
            .with_route_endpoint(endpoint),
    );
    let a = seed_actors(&app).await;
    let classroom: Uuid = a.classroom.parse().unwrap();
    let owner: Uuid = sqlx::query_scalar("SELECT created_by FROM classrooms WHERE id=$1")
        .bind(classroom)
        .fetch_one(&pool)
        .await
        .unwrap();
    let conv = start_conversation(&app, &a).await;
    World {
        app,
        a,
        pool,
        fake,
        system,
        owner,
        classroom,
        conv,
    }
}

impl World {
    async fn add_key(&self, owner: Uuid, provider: &str, key: &str) -> Uuid {
        keys::add(
            &self.pool,
            &KeyCipher::parse(ENC).unwrap(),
            owner,
            provider,
            key,
        )
        .await
        .unwrap()
        .id
    }
    async fn configure(&self) {
        classroom_ai_store::set(
            &self.pool,
            self.classroom,
            ModelRef {
                provider: "openai".into(),
                model: MODEL.into(),
            },
            vec![],
        )
        .await
        .unwrap();
    }
    async fn say(&self) -> Vec<(String, Value)> {
        let (_, events) =
            student_says_events(&self.app, &self.a, &self.conv, "我覺得應該拉桿").await;
        events.expect("SSE stream")
    }
    async fn status_of(&self, id: Uuid) -> KeyStatus {
        keys::list_for_owner(&self.pool, self.owner)
            .await
            .unwrap()
            .into_iter()
            .find(|k| k.id == id)
            .unwrap()
            .status
    }
}

fn error_code(events: &[(String, Value)]) -> Option<String> {
    events
        .iter()
        .find(|(n, _)| n == "error")
        .and_then(|(_, d)| d["code"].as_str().map(str::to_owned))
}

fn reply_text(events: &[(String, Value)]) -> String {
    events
        .iter()
        .filter(|(n, _)| n == "delta")
        .filter_map(|(_, d)| d["text"].as_str().or(d["content"].as_str()))
        .collect()
}

async fn wait_usage(w: &World, key: Uuid, requests: i64) -> usage::Totals {
    for _ in 0..200 {
        let t = usage::totals_for_key(&w.pool, key, Range::All)
            .await
            .unwrap();
        if t.requests >= requests {
            return t;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("usage was not recorded");
}

#[sqlx::test]
async fn configured_classroom_replies_with_the_creators_key_and_model_not_the_system_key(
    pool: PgPool,
) {
    let w = world(pool).await;
    let key = w.add_key(w.owner, "openai", KEY_1).await;
    w.configure().await;
    let events = w.say().await;
    assert_eq!(error_code(&events), None, "{events:?}");
    assert!(events.iter().any(|(n, _)| n == "done"));
    let seen = w.fake.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        (seen[0].key.as_str(), seen[0].model.as_str()),
        (KEY_1, MODEL)
    );
    assert_eq!(w.system.request_count(), 0, "不得使用系統 key");
    // 用量：一次請求與供應商回報的 token，記在實際模型上
    let t = wait_usage(&w, key, 1).await;
    assert_eq!(
        (
            t.requests,
            t.input_tokens,
            t.output_tokens,
            t.unreported_requests
        ),
        (1, 11, 7, 0)
    );
    let model: String = sqlx::query_scalar("SELECT model FROM teacher_key_usage WHERE key_id=$1")
        .bind(key)
        .fetch_one(&w.pool)
        .await
        .unwrap();
    assert_eq!(model, MODEL);
}

#[sqlx::test]
async fn classroom_without_a_setting_or_key_is_unavailable_and_never_falls_back_to_the_system_key(
    pool: PgPool,
) {
    let w = world(pool).await;
    // 沒設定
    assert_eq!(
        error_code(&w.say().await).as_deref(),
        Some("classroom_ai_unavailable")
    );
    // 有設定但沒有 key
    w.configure().await;
    assert_eq!(
        error_code(&w.say().await).as_deref(),
        Some("classroom_ai_unavailable")
    );
    assert_eq!(w.system.request_count(), 0);
    assert!(w.fake.keys_seen().is_empty());
}

#[sqlx::test]
async fn transitional_classroom_keeps_using_the_system_key(pool: PgPool) {
    let w = world(pool.clone()).await;
    sqlx::query("UPDATE classrooms SET ai_transitional = true")
        .execute(&pool)
        .await
        .unwrap();
    let events = w.say().await;
    assert_eq!(error_code(&events), None, "{events:?}");
    assert_eq!(w.system.request_count(), 1);
    assert!(w.fake.keys_seen().is_empty());
    // 清除過渡旗標後不再退回系統 key
    classroom_ai_store::clear_transitional(&pool, w.classroom)
        .await
        .unwrap();
    assert_eq!(
        error_code(&w.say().await).as_deref(),
        Some("classroom_ai_unavailable")
    );
    assert_eq!(w.system.request_count(), 1);
}

#[sqlx::test]
async fn keys_rotate_between_calls(pool: PgPool) {
    let w = world(pool).await;
    w.add_key(w.owner, "openai", KEY_1).await;
    w.add_key(w.owner, "openai", KEY_2).await;
    w.configure().await;
    for _ in 0..2 {
        assert_eq!(error_code(&w.say().await), None);
    }
    let seen = w.fake.keys_seen();
    assert_eq!(seen.len(), 2);
    assert_ne!(seen[0], seen[1], "輪流使用：{seen:?}");
}

#[sqlx::test]
async fn rejected_limited_or_exhausted_key_is_recorded_and_the_next_key_is_tried(pool: PgPool) {
    for (behavior, expected) in [
        (
            Behavior::Status(401, r#"{"error":{"message":"bad key"}}"#),
            KeyStatus::Invalid,
        ),
        (
            Behavior::Status(429, r#"{"error":{"message":"slow down"}}"#),
            KeyStatus::RateLimited,
        ),
        (
            Behavior::Status(429, r#"{"error":{"code":"insufficient_quota"}}"#),
            KeyStatus::QuotaExhausted,
        ),
        (Behavior::Status(402, "{}"), KeyStatus::QuotaExhausted),
    ] {
        let w = world(pool_clone(&pool).await).await;
        let bad = w.add_key(w.owner, "openai", KEY_1).await;
        let good = w.add_key(w.owner, "openai", KEY_2).await;
        w.configure().await;
        w.fake.set(KEY_1, behavior);
        let events = w.say().await;
        assert_eq!(error_code(&events), None, "{events:?}");
        assert!(reply_text(&events).contains("請說說你的理由"), "{events:?}");
        // 不論輪到哪把先，最後一定由好的那把回覆
        assert_eq!(w.fake.keys_seen().last().map(String::as_str), Some(KEY_2));
        assert_eq!(w.status_of(good).await, KeyStatus::Ok);
        if w.fake.keys_seen().contains(&KEY_1.to_string()) {
            assert_eq!(w.status_of(bad).await, expected);
        }
        assert_eq!(w.system.request_count(), 0);
    }
}

/// `#[sqlx::test]` 每個測試只給一個資料庫；迴圈裡要重新開始時清掉上一輪的資料。
async fn pool_clone(pool: &PgPool) -> PgPool {
    sqlx::query("TRUNCATE users, classrooms, teachers RESTART IDENTITY CASCADE")
        .execute(pool)
        .await
        .unwrap();
    pool.clone()
}

#[sqlx::test]
async fn failing_key_enters_cooldown_then_gets_another_chance(pool: PgPool) {
    let w = world(pool.clone()).await;
    let bad = w.add_key(w.owner, "openai", KEY_1).await;
    w.configure().await;
    w.fake.set(KEY_1, Behavior::Status(401, "{}"));
    // 只有一把且失效：整個教室不可用
    assert_eq!(
        error_code(&w.say().await).as_deref(),
        Some("classroom_ai_unavailable")
    );
    assert_eq!(w.status_of(bad).await, KeyStatus::Invalid);
    let before = w.fake.keys_seen().len();
    // 冷卻中不再打擾供應商
    assert_eq!(
        error_code(&w.say().await).as_deref(),
        Some("classroom_ai_unavailable")
    );
    assert_eq!(w.fake.keys_seen().len(), before);
    // 冷卻過後再給一次機會；這次成功，狀態恢復
    w.fake.set(KEY_1, Behavior::Ok);
    sqlx::query("UPDATE teacher_api_keys SET status_at = now() - interval '11 minutes'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(error_code(&w.say().await), None);
    assert_eq!(w.status_of(bad).await, KeyStatus::Ok);
}

#[sqlx::test]
async fn other_errors_do_not_switch_keys_or_change_key_status(pool: PgPool) {
    let w = world(pool).await;
    let k1 = w.add_key(w.owner, "openai", KEY_1).await;
    w.configure().await;
    w.fake.set(
        KEY_1,
        Behavior::Status(500, r#"{"error":"PROVIDERBODY-SENTINEL"}"#),
    );
    let events = w.say().await;
    assert_eq!(error_code(&events).as_deref(), Some("ai_unavailable"));
    assert_eq!(w.status_of(k1).await, KeyStatus::Ok);
    assert!(!format!("{events:?}").contains("PROVIDERBODY-SENTINEL"));
    // 失敗的請求也記一次（供應商沒回報 token）
    let t = wait_usage(&w, k1, 1).await;
    assert!(t.requests >= 1 && t.unreported_requests >= 1);
}

#[sqlx::test]
async fn disabled_creator_or_removed_teacher_role_stops_ai_until_restored(pool: PgPool) {
    let w = world(pool.clone()).await;
    w.add_key(w.owner, "openai", KEY_1).await;
    w.configure().await;
    sqlx::query("UPDATE users SET disabled_at = now() WHERE id = $1")
        .bind(w.owner)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        error_code(&w.say().await).as_deref(),
        Some("classroom_ai_unavailable")
    );
    sqlx::query("UPDATE users SET disabled_at = NULL WHERE id = $1")
        .bind(w.owner)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(error_code(&w.say().await), None);
    // 教師身分被移除
    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id=$1")
        .bind(w.owner)
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM teachers WHERE email = $1")
        .bind(&email)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        error_code(&w.say().await).as_deref(),
        Some("classroom_ai_unavailable")
    );
    sqlx::query("INSERT INTO teachers (email) VALUES ($1)")
        .bind(&email)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(error_code(&w.say().await), None);
    assert_eq!(w.system.request_count(), 0);
}

#[sqlx::test]
async fn another_teachers_key_is_never_used_for_this_classroom(pool: PgPool) {
    let w = world(pool.clone()).await;
    let other = seed_user(&pool, "other-teacher@example.com").await;
    w.add_key(other, "openai", OTHER_TEACHER_KEY).await;
    w.configure().await;
    // 建立者自己沒有 key：不能借用別人的
    assert_eq!(
        error_code(&w.say().await).as_deref(),
        Some("classroom_ai_unavailable")
    );
    assert!(w.fake.keys_seen().is_empty());
    // 建立者的 key 只在同供應商才算
    w.add_key(w.owner, "anthropic", KEY_2).await;
    assert_eq!(
        error_code(&w.say().await).as_deref(),
        Some("classroom_ai_unavailable")
    );
    assert!(w.fake.keys_seen().is_empty());
}
