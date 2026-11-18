//! 管理者在介面管理系統 AI（供應商、模型、系統 key）；所有資料皆為合成。
mod common;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::{Router, http::StatusCode};
use common::*;
use serde_json::{Value, json};
use socrates_chat_backend::{
    AppState,
    ai_providers::{KeyCheck, KeyVerifier, ModelCheckError, ModelVerifier, Provider},
    app,
    teacher_key_crypto::KeyCipher,
};
use sqlx::PgPool;

const ENC: &str = "k1:MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";
const KEY_A: &str = "sk-synthetic-AAAA-1111-2222-3333-aaaa";
const KEY_B: &str = "sk-synthetic-BBBB-1111-2222-3333-bbbb";
const TEACHER_KEY: &str = "sk-synthetic-TTTT-1111-2222-3333-tttt";

#[derive(Default)]
struct Fake {
    key_result: Mutex<Option<KeyCheck>>,
    model_failure: Mutex<Option<ModelCheckError>>,
    model_calls: Mutex<Vec<(String, String, String)>>,
}

#[async_trait]
impl KeyVerifier for Fake {
    async fn verify_key(&self, _: &Provider, _: &str) -> KeyCheck {
        self.key_result.lock().unwrap().unwrap_or(KeyCheck::Usable)
    }
}

#[async_trait]
impl ModelVerifier for Fake {
    async fn verify_model(
        &self,
        p: &Provider,
        model: &str,
        key: &str,
    ) -> Result<(), ModelCheckError> {
        self.model_calls
            .lock()
            .unwrap()
            .push((p.id.into(), model.into(), key.into()));
        match *self.model_failure.lock().unwrap() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

fn build(pool: PgPool, with_cipher: bool, fake: Arc<Fake>) -> Router {
    register_fixture_pool(&pool);
    let mut config = test_config();
    if with_cipher {
        config.teacher_key_cipher = Some(Arc::new(KeyCipher::parse(ENC).unwrap()));
        config.classroom_ai_routing = true;
    }
    app(
        AppState::new(pool, config, Arc::new(FakeIdentity), FakeAi::with(vec![]))
            .with_key_verifier(fake.clone())
            .with_model_verifier(fake),
    )
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let res = send(app, json_req(method, path, cookie, body)).await;
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn code(v: &Value) -> &str {
    v["error"]["code"].as_str().unwrap_or("")
}

const BASE: &str = "/api/admin/system-ai";

async fn add(app: &Router, cookie: &str, provider: &str, key: &str) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        &format!("{BASE}/keys"),
        Some(cookie),
        Some(json!({"provider": provider, "key": key})),
    )
    .await
}

async fn audit_actions(pool: &PgPool) -> Vec<(String, String)> {
    sqlx::query_as("SELECT action, target_email FROM audit_logs WHERE action LIKE 'system_%' ORDER BY created_at, id")
        .fetch_all(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn admin_adds_replaces_tests_and_deletes_system_keys_without_leaking_them(pool: PgPool) {
    let fake = Arc::new(Fake::default());
    let app = build(pool.clone(), true, fake.clone());
    let a = seed_actors(&app).await;
    let (s, item) = add(&app, &a.admin, "openai", KEY_A).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(
        (
            item["provider"].as_str(),
            item["mask"].as_str(),
            item["status"].as_str()
        ),
        (Some("openai"), Some("…aaaa"), Some("ok"))
    );
    let id = item["id"].as_str().unwrap().to_owned();
    let (_, o) = call(&app, "GET", BASE, Some(&a.admin), None).await;
    assert_eq!(o["source"], "host_file");
    assert_eq!(o["keys"].as_array().unwrap().len(), 1);
    assert!(!format!("{item}{o}").contains(KEY_A));
    let stored: String = sqlx::query_scalar("SELECT t::text FROM system_api_keys t")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!stored.contains(KEY_A));

    // 更換：id 與供應商不變，狀態重設
    sqlx::query("UPDATE system_api_keys SET status = 'invalid', status_at = now()")
        .execute(&pool)
        .await
        .unwrap();
    let (s, r) = call(
        &app,
        "PUT",
        &format!("{BASE}/keys/{id}"),
        Some(&a.admin),
        Some(json!({"key": KEY_B})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        (r["id"].as_str(), r["mask"].as_str(), r["status"].as_str()),
        (Some(id.as_str()), Some("…bbbb"), Some("ok"))
    );

    // 測試：不可用保留狀態，可用清掉狀態
    sqlx::query("UPDATE system_api_keys SET status = 'rate_limited', status_at = now()")
        .execute(&pool)
        .await
        .unwrap();
    *fake.key_result.lock().unwrap() = Some(KeyCheck::ConnectionFailed);
    let (_, t) = call(
        &app,
        "POST",
        &format!("{BASE}/keys/{id}/test"),
        Some(&a.admin),
        None,
    )
    .await;
    assert_eq!(t["result"], "connection_failed");
    let status: String = sqlx::query_scalar("SELECT status FROM system_api_keys")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "rate_limited");
    *fake.key_result.lock().unwrap() = Some(KeyCheck::Usable);
    let (_, t) = call(
        &app,
        "POST",
        &format!("{BASE}/keys/{id}/test"),
        Some(&a.admin),
        None,
    )
    .await;
    assert_eq!(t["result"], "usable");
    let status: String = sqlx::query_scalar("SELECT status FROM system_api_keys")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "ok");

    let (s, _) = call(
        &app,
        "DELETE",
        &format!("{BASE}/keys/{id}"),
        Some(&a.admin),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, e) = call(
        &app,
        "DELETE",
        &format!("{BASE}/keys/{id}"),
        Some(&a.admin),
        None,
    )
    .await;
    assert_eq!((s, code(&e)), (StatusCode::NOT_FOUND, "not_found"));
    // 稽核只記操作與供應商識別碼
    assert_eq!(
        audit_actions(&pool).await,
        vec![
            ("system_key_added".into(), "openai".into()),
            ("system_key_replaced".into(), "openai".into()),
            ("system_key_deleted".into(), "openai".into()),
        ]
    );
}

#[sqlx::test]
async fn validation_and_duplicates_cover_teacher_keys_too(pool: PgPool) {
    let app = build(pool.clone(), true, Arc::new(Fake::default()));
    let a = seed_actors(&app).await;
    let (s, e) = add(&app, &a.admin, "ollama", KEY_A).await;
    assert_eq!((s, code(&e)), (StatusCode::BAD_REQUEST, "invalid_provider"));
    for bad in ["", "has space", &"x".repeat(513)] {
        let (s, e) = add(&app, &a.admin, "openai", bad).await;
        assert_eq!(
            (s, code(&e)),
            (StatusCode::BAD_REQUEST, "invalid_key"),
            "{bad:?}"
        );
    }
    assert_eq!(
        add(&app, &a.admin, "openai", KEY_A).await.0,
        StatusCode::CREATED
    );
    let (s, e) = add(&app, &a.admin, "anthropic", KEY_A).await;
    assert_eq!((s, code(&e)), (StatusCode::CONFLICT, "key_already_exists"));
    // 教師已綁定的 key 不能再當系統 key
    call(
        &app,
        "POST",
        "/api/teacher/keys",
        Some(&a.teacher),
        Some(json!({"provider":"openai","key":TEACHER_KEY})),
    )
    .await;
    let (s, e) = add(&app, &a.admin, "openai", TEACHER_KEY).await;
    assert_eq!((s, code(&e)), (StatusCode::CONFLICT, "key_already_exists"));
    // 沒有 key 的 id 一律 404
    for p in [
        format!("{BASE}/keys/00000000-0000-0000-0000-000000000000/test"),
        format!("{BASE}/keys/not-a-uuid/test"),
    ] {
        assert_eq!(
            call(&app, "POST", &p, Some(&a.admin), None).await.0,
            StatusCode::NOT_FOUND
        );
    }
}

#[sqlx::test]
async fn model_is_verified_with_a_system_key_before_it_is_saved_and_can_be_cleared(pool: PgPool) {
    let fake = Arc::new(Fake::default());
    let app = build(pool.clone(), true, fake.clone());
    let a = seed_actors(&app).await;
    let put = |model: &str| json!({"provider": "openai", "model": model});
    // 沒有該供應商的 key
    let (s, e) = call(
        &app,
        "PUT",
        &format!("{BASE}/model"),
        Some(&a.admin),
        Some(put("gpt-x")),
    )
    .await;
    assert_eq!(
        (s, code(&e)),
        (StatusCode::BAD_REQUEST, "no_key_for_provider")
    );
    add(&app, &a.admin, "openai", KEY_A).await;
    // 驗證失敗：不儲存
    *fake.model_failure.lock().unwrap() = Some(ModelCheckError::ModelNotFound);
    let (s, e) = call(
        &app,
        "PUT",
        &format!("{BASE}/model"),
        Some(&a.admin),
        Some(put("nope")),
    )
    .await;
    assert_eq!((s, code(&e)), (StatusCode::BAD_REQUEST, "model_not_found"));
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM system_ai_settings")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    // 通過：儲存並改用資料庫的設定
    *fake.model_failure.lock().unwrap() = None;
    let (s, o) = call(
        &app,
        "PUT",
        &format!("{BASE}/model"),
        Some(&a.admin),
        Some(put("  gpt-x  ")),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        (o["source"].as_str(), o["setting"]["model"].as_str()),
        (Some("database"), Some("gpt-x"))
    );
    assert_eq!(
        fake.model_calls.lock().unwrap().last().cloned(),
        Some(("openai".into(), "gpt-x".into(), KEY_A.into()))
    );
    for bad in [
        json!({"provider":"nope","model":"m"}),
        json!({"provider":"openai","model":" "}),
        json!({"provider":"openai","model":"x".repeat(201)}),
    ] {
        assert_eq!(
            call(
                &app,
                "PUT",
                &format!("{BASE}/model"),
                Some(&a.admin),
                Some(bad)
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    // 清除：回到主機的 key，key 保留
    let (s, o) = call(
        &app,
        "DELETE",
        &format!("{BASE}/model"),
        Some(&a.admin),
        None,
    )
    .await;
    assert_eq!(
        (s, o["source"].as_str(), o["keys"].as_array().map(Vec::len)),
        (StatusCode::OK, Some("host_file"), Some(1))
    );
    assert_eq!(
        audit_actions(&pool)
            .await
            .iter()
            .map(|(a, _)| a.as_str())
            .collect::<Vec<_>>(),
        [
            "system_key_added",
            "system_ai_model_set",
            "system_ai_model_cleared"
        ]
    );
}

#[sqlx::test]
async fn only_admins_can_manage_system_ai(pool: PgPool) {
    let fake = Arc::new(Fake::default());
    let app = build(pool.clone(), true, fake.clone());
    let a = seed_actors(&app).await;
    let (_, item) = add(&app, &a.admin, "openai", KEY_A).await;
    let id = item["id"].as_str().unwrap();
    let calls: Vec<(&str, String, Option<Value>)> = vec![
        ("GET", BASE.into(), None),
        (
            "PUT",
            format!("{BASE}/model"),
            Some(json!({"provider":"openai","model":"m"})),
        ),
        ("DELETE", format!("{BASE}/model"), None),
        (
            "POST",
            format!("{BASE}/keys"),
            Some(json!({"provider":"openai","key":KEY_B})),
        ),
        (
            "PUT",
            format!("{BASE}/keys/{id}"),
            Some(json!({"key": KEY_B})),
        ),
        ("DELETE", format!("{BASE}/keys/{id}"), None),
        ("POST", format!("{BASE}/keys/{id}/test"), None),
    ];
    for (m, p, b) in &calls {
        let (s, e) = call(&app, m, p, None, b.clone()).await;
        assert_eq!(
            (s, code(&e)),
            (StatusCode::UNAUTHORIZED, "unauthorized"),
            "{m} {p}"
        );
        for cookie in [&a.teacher, &a.student, &a.outsider] {
            assert_eq!(
                call(&app, m, p, Some(cookie), b.clone()).await.0,
                StatusCode::FORBIDDEN,
                "{m} {p}"
            );
        }
    }
    // 沒有任何副作用
    assert!(fake.model_calls.lock().unwrap().is_empty());
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM system_api_keys")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
    // 教師 key 的管理者刪除端點與教師的 key 清單都碰不到系統 key
    let (s, _) = call(
        &app,
        "DELETE",
        &format!("/api/admin/teacher-keys/{id}"),
        Some(&a.admin),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (_, mine) = call(&app, "GET", "/api/teacher/keys", Some(&a.teacher), None).await;
    assert_eq!(mine.as_array().unwrap().len(), 0);
}

#[sqlx::test]
async fn csrf_origin_is_required(pool: PgPool) {
    let app = build(pool.clone(), true, Arc::new(Fake::default()));
    let a = seed_actors(&app).await;
    let req = axum::http::Request::builder()
        .method("POST")
        .uri(format!("{BASE}/keys"))
        .header(axum::http::header::COOKIE, a.admin.clone())
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            json!({"provider":"openai","key":KEY_A}).to_string(),
        ))
        .unwrap();
    assert_eq!(send(&app, req).await.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn without_an_encryption_key_keys_cannot_be_added(pool: PgPool) {
    let app = build(pool, false, Arc::new(Fake::default()));
    let a = seed_actors(&app).await;
    let (s, e) = add(&app, &a.admin, "openai", KEY_A).await;
    assert_eq!(
        (s, code(&e)),
        (StatusCode::SERVICE_UNAVAILABLE, "teacher_keys_unavailable")
    );
    let (s, o) = call(&app, "GET", BASE, Some(&a.admin), None).await;
    assert_eq!(
        (
            s,
            o["encryption_configured"].as_bool(),
            o["routing_enabled"].as_bool()
        ),
        (StatusCode::OK, Some(false), Some(false))
    );
}
