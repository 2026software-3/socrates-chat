//! F-40.1-BE (#592): 教師管理自己的 AI 供應商 API key；所有 key 皆為合成字串。
mod common;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::{Router, http::StatusCode};
use common::*;
use serde_json::{Value, json};
use socrates_chat_backend::{
    AppState,
    ai_providers::{KeyCheck, KeyVerifier, ListUnavailable, ModelLister, Provider},
    app,
    teacher_key_crypto::KeyCipher,
};
use sqlx::PgPool;

const ENC: &str = "k1:MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";
const KEY_A: &str = "sk-synthetic-AAAA-1111-2222-3333-aaaa";
const KEY_B: &str = "sk-synthetic-BBBB-1111-2222-3333-bbbb";
const KEY_C: &str = "sk-synthetic-CCCC-1111-2222-3333-cccc";

struct FakeVerifier {
    result: Mutex<KeyCheck>,
    calls: Mutex<Vec<(String, String)>>,
    delay: Mutex<std::time::Duration>,
}

#[async_trait]
impl KeyVerifier for FakeVerifier {
    async fn verify_key(&self, provider: &Provider, key: &str) -> KeyCheck {
        self.calls
            .lock()
            .unwrap()
            .push((provider.id.into(), key.into()));
        let delay = *self.delay.lock().unwrap();
        tokio::time::sleep(delay).await;
        *self.result.lock().unwrap()
    }
}

fn verifier(result: KeyCheck) -> Arc<FakeVerifier> {
    Arc::new(FakeVerifier {
        result: Mutex::new(result),
        calls: Mutex::default(),
        delay: Mutex::default(),
    })
}

fn build(pool: PgPool, with_cipher: bool, v: Arc<FakeVerifier>, limit: Option<usize>) -> Router {
    register_fixture_pool(&pool);
    let mut config = test_config();
    if with_cipher {
        config.teacher_key_cipher = Some(Arc::new(KeyCipher::parse(ENC).unwrap()));
        config.classroom_ai_routing = true;
    }
    let mut state = AppState::new(pool, config, Arc::new(FakeIdentity), FakeAi::with(vec![]))
        .with_key_verifier(v);
    if let Some(max) = limit {
        state = state.with_key_test_limit(max, std::time::Duration::from_secs(600));
    }
    app(state)
}

fn setup(pool: PgPool) -> (Router, Arc<FakeVerifier>) {
    let v = verifier(KeyCheck::Usable);
    (build(pool, true, v.clone(), None), v)
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
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn add(app: &Router, cookie: &str, provider: &str, key: &str) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/api/teacher/keys",
        Some(cookie),
        Some(json!({"provider": provider, "key": key})),
    )
    .await
}

async fn second_teacher(app: &Router, admin: &str) -> String {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(admin),
            Some(json!({"email": "t2@example.com"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    login_as(app, "sub-t2", "t2@example.com").await
}

fn code(v: &Value) -> &str {
    v["error"]["code"].as_str().unwrap_or("")
}

#[sqlx::test]
async fn teacher_adds_and_lists_keys_with_mask_only(pool: PgPool) {
    let (app, _) = setup(pool.clone());
    let a = seed_actors(&app).await;
    let (status, item) = add(&app, &a.teacher, "openai", KEY_A).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(item["provider"], "openai");
    assert_eq!(item["mask"], "…aaaa");
    assert_eq!(item["status"], "ok");
    assert!(item["status_at"].is_null());
    let (status, list) = call(&app, "GET", "/api/teacher/keys", Some(&a.teacher), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["id"], item["id"]);
    // 完整 key 與密文絕不回傳
    let dump = format!("{item}{list}");
    assert!(!dump.contains(KEY_A) && !dump.contains("ciphertext") && !dump.contains("fingerprint"));
    // 資料庫裡也沒有明文
    let stored: String = sqlx::query_scalar("SELECT t::text FROM teacher_api_keys t")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!stored.contains(KEY_A));
}

#[sqlx::test]
async fn provider_list_comes_from_the_pinned_catalog(pool: PgPool) {
    let (app, _) = setup(pool);
    let a = seed_actors(&app).await;
    let (status, list) = call(
        &app,
        "GET",
        "/api/teacher/ai-providers",
        Some(&a.teacher),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.first(), Some(&"openai"));
    assert!(ids.contains(&"anthropic") && !ids.contains(&"ollama"));
    assert!(list[0]["name"].is_string());
    assert!(list[0].get("endpoint").is_none());
}

#[sqlx::test]
async fn add_validates_provider_and_key(pool: PgPool) {
    let (app, _) = setup(pool);
    let a = seed_actors(&app).await;
    for provider in ["nope", "OpenAI", "ollama", ""] {
        let (s, b) = add(&app, &a.teacher, provider, KEY_A).await;
        assert_eq!(
            (s, code(&b)),
            (StatusCode::BAD_REQUEST, "invalid_provider"),
            "{provider}"
        );
    }
    let too_long = "k".repeat(513);
    for key in [
        "",
        "   ",
        "has space inside-key-value",
        "line\nbreak-key-value-xx",
        too_long.as_str(),
    ] {
        let (s, b) = add(&app, &a.teacher, "openai", key).await;
        assert_eq!(
            (s, code(&b)),
            (StatusCode::BAD_REQUEST, "invalid_key"),
            "{key:?}"
        );
    }
    // 前後空白會去掉
    let (s, item) = add(&app, &a.teacher, "openai", &format!("  {KEY_A}\n")).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(item["mask"], "…aaaa");
    // 缺欄位
    let (s, _) = call(
        &app,
        "POST",
        "/api/teacher/keys",
        Some(&a.teacher),
        Some(json!({"provider": "openai"})),
    )
    .await;
    assert!(s.is_client_error());
}

#[sqlx::test]
async fn duplicate_key_returns_409_even_across_teachers_without_echoing_it(pool: PgPool) {
    let (app, _) = setup(pool);
    let a = seed_actors(&app).await;
    let t2 = second_teacher(&app, &a.admin).await;
    assert_eq!(
        add(&app, &a.teacher, "openai", KEY_A).await.0,
        StatusCode::CREATED
    );
    for cookie in [&a.teacher, &t2] {
        let (s, b) = add(&app, cookie, "openai", KEY_A).await;
        assert_eq!((s, code(&b)), (StatusCode::CONFLICT, "key_already_exists"));
        assert!(!b.to_string().contains(KEY_A));
    }
    assert_eq!(add(&app, &t2, "openai", KEY_B).await.0, StatusCode::CREATED);
}

#[sqlx::test]
async fn replace_keeps_id_and_provider_and_provider_cannot_change(pool: PgPool) {
    let (app, _) = setup(pool);
    let a = seed_actors(&app).await;
    let (_, item) = add(&app, &a.teacher, "openai", KEY_A).await;
    let path = format!("/api/teacher/keys/{}", item["id"].as_str().unwrap());
    let (s, r) = call(
        &app,
        "PUT",
        &path,
        Some(&a.teacher),
        Some(json!({"key": KEY_B})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        (&r["id"], &r["provider"], &r["mask"]),
        (&item["id"], &item["provider"], &json!("…bbbb"))
    );
    assert!(!r.to_string().contains(KEY_B));
    // 要換供應商要新增另一把：帶 provider 的請求被拒絕
    let (s, _) = call(
        &app,
        "PUT",
        &path,
        Some(&a.teacher),
        Some(json!({"key": KEY_C, "provider": "anthropic"})),
    )
    .await;
    assert!(s.is_client_error());
    let (_, list) = call(&app, "GET", "/api/teacher/keys", Some(&a.teacher), None).await;
    assert_eq!(
        (&list[0]["provider"], &list[0]["mask"]),
        (&json!("openai"), &json!("…bbbb"))
    );
    // 換成已存在的 key、無效的 key
    add(&app, &a.teacher, "openai", KEY_C).await;
    let (s, b) = call(
        &app,
        "PUT",
        &path,
        Some(&a.teacher),
        Some(json!({"key": KEY_C})),
    )
    .await;
    assert_eq!((s, code(&b)), (StatusCode::CONFLICT, "key_already_exists"));
    let (s, b) = call(
        &app,
        "PUT",
        &path,
        Some(&a.teacher),
        Some(json!({"key": "  "})),
    )
    .await;
    assert_eq!((s, code(&b)), (StatusCode::BAD_REQUEST, "invalid_key"));
}

#[sqlx::test]
async fn delete_removes_the_key_and_it_can_be_bound_again(pool: PgPool) {
    let (app, _) = setup(pool);
    let a = seed_actors(&app).await;
    let (_, item) = add(&app, &a.teacher, "openai", KEY_A).await;
    let path = format!("/api/teacher/keys/{}", item["id"].as_str().unwrap());
    assert_eq!(
        call(&app, "DELETE", &path, Some(&a.teacher), None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, "DELETE", &path, Some(&a.teacher), None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        add(&app, &a.teacher, "openai", KEY_A).await.0,
        StatusCode::CREATED
    );
}

#[sqlx::test]
async fn list_shows_status_reason_and_time(pool: PgPool) {
    let (app, _) = setup(pool.clone());
    let a = seed_actors(&app).await;
    let (_, item) = add(&app, &a.teacher, "openai", KEY_A).await;
    sqlx::query("UPDATE teacher_api_keys SET status='quota_exhausted', status_at=now()")
        .execute(&pool)
        .await
        .unwrap();
    let (_, list) = call(&app, "GET", "/api/teacher/keys", Some(&a.teacher), None).await;
    assert_eq!(list[0]["id"], item["id"]);
    assert_eq!(list[0]["status"], "quota_exhausted");
    assert!(list[0]["status_at"].is_string());
}

#[sqlx::test]
async fn test_endpoint_reports_usable_or_connection_failed_without_leaking_the_key(pool: PgPool) {
    let v = verifier(KeyCheck::Usable);
    let app = build(pool, true, v.clone(), None);
    let a = seed_actors(&app).await;
    let (_, item) = add(&app, &a.teacher, "anthropic", KEY_A).await;
    let path = format!("/api/teacher/keys/{}/test", item["id"].as_str().unwrap());
    let (s, r) = call(&app, "POST", &path, Some(&a.teacher), None).await;
    assert_eq!((s, r["result"].as_str()), (StatusCode::OK, Some("usable")));
    assert_eq!(
        v.calls.lock().unwrap().clone(),
        vec![("anthropic".to_string(), KEY_A.to_string())]
    );
    *v.result.lock().unwrap() = KeyCheck::ConnectionFailed;
    let (s, r) = call(&app, "POST", &path, Some(&a.teacher), None).await;
    assert_eq!(
        (s, r["result"].as_str()),
        (StatusCode::OK, Some("connection_failed"))
    );
    assert!(!r.to_string().contains(KEY_A));
}

#[sqlx::test]
async fn usable_test_clears_a_recorded_failure_but_connection_failure_keeps_it(pool: PgPool) {
    let v = verifier(KeyCheck::ConnectionFailed);
    let app = build(pool.clone(), true, v.clone(), None);
    let a = seed_actors(&app).await;
    let (_, item) = add(&app, &a.teacher, "openai", KEY_A).await;
    sqlx::query("UPDATE teacher_api_keys SET status='invalid', status_at=now()")
        .execute(&pool)
        .await
        .unwrap();
    let path = format!("/api/teacher/keys/{}/test", item["id"].as_str().unwrap());
    let status = || async {
        let (_, list) = call(&app, "GET", "/api/teacher/keys", Some(&a.teacher), None).await;
        list[0]["status"].as_str().unwrap().to_owned()
    };
    // 連線失敗可能只是網路問題：不改寫狀態
    call(&app, "POST", &path, Some(&a.teacher), None).await;
    assert_eq!(status().await, "invalid");
    // 教師補足額度後測試可用：狀態清為可用，路由不必等冷卻結束
    *v.result.lock().unwrap() = KeyCheck::Usable;
    let (_, r) = call(&app, "POST", &path, Some(&a.teacher), None).await;
    assert_eq!(r["result"], "usable");
    assert_eq!(status().await, "ok");
}

#[sqlx::test]
async fn a_listed_key_that_can_no_longer_be_decrypted_tests_as_connection_failed(pool: PgPool) {
    let v = verifier(KeyCheck::Usable);
    let app = build(pool.clone(), true, v.clone(), None);
    let a = seed_actors(&app).await;
    let (_, item) = add(&app, &a.teacher, "openai", KEY_A).await;
    // 以已輪替掉的加密金鑰保存的 key：清單看得到，但解不開
    sqlx::query("UPDATE teacher_api_keys SET encryption_key_id = 'retired'")
        .execute(&pool)
        .await
        .unwrap();
    let path = format!("/api/teacher/keys/{}/test", item["id"].as_str().unwrap());
    let (s, r) = call(&app, "POST", &path, Some(&a.teacher), None).await;
    assert_eq!(
        (s, r["result"].as_str()),
        (StatusCode::OK, Some("connection_failed"))
    );
    assert!(v.calls.lock().unwrap().is_empty());
}

#[sqlx::test]
async fn a_slow_provider_test_is_not_cut_off_by_the_general_request_timeout(pool: PgPool) {
    // 登入（密碼雜湊）用一般逾時的 app 準備資料；session 存在資料庫，換 app 照樣有效
    let (seed, _) = setup(pool.clone());
    let a = seed_actors(&seed).await;
    let (_, item) = add(&seed, &a.teacher, "openai", KEY_A).await;
    let v = verifier(KeyCheck::Usable);
    *v.delay.lock().unwrap() = std::time::Duration::from_millis(400);
    let mut config = test_config();
    config.teacher_key_cipher = Some(Arc::new(KeyCipher::parse(ENC).unwrap()));
    config.request_timeout = std::time::Duration::from_millis(100);
    let app = app(
        AppState::new(pool, config, Arc::new(FakeIdentity), FakeAi::with(vec![]))
            .with_key_verifier(v),
    );
    let path = format!("/api/teacher/keys/{}/test", item["id"].as_str().unwrap());
    let (s, r) = call(&app, "POST", &path, Some(&a.teacher), None).await;
    assert_eq!((s, r["result"].as_str()), (StatusCode::OK, Some("usable")));
}

#[sqlx::test]
async fn test_endpoint_is_rate_limited_per_teacher(pool: PgPool) {
    let v = verifier(KeyCheck::Usable);
    let app = build(pool, true, v.clone(), Some(2));
    let a = seed_actors(&app).await;
    let t2 = second_teacher(&app, &a.admin).await;
    let (_, mine) = add(&app, &a.teacher, "openai", KEY_A).await;
    let (_, theirs) = add(&app, &t2, "openai", KEY_B).await;
    let mine = format!("/api/teacher/keys/{}/test", mine["id"].as_str().unwrap());
    let theirs = format!("/api/teacher/keys/{}/test", theirs["id"].as_str().unwrap());
    for _ in 0..2 {
        assert_eq!(
            call(&app, "POST", &mine, Some(&a.teacher), None).await.0,
            StatusCode::OK
        );
    }
    let (s, b) = call(&app, "POST", &mine, Some(&a.teacher), None).await;
    assert_eq!(
        (s, code(&b)),
        (StatusCode::TOO_MANY_REQUESTS, "key_test_rate_limited")
    );
    assert_eq!(v.calls.lock().unwrap().len(), 2);
    // 別的教師不受影響
    assert_eq!(
        call(&app, "POST", &theirs, Some(&t2), None).await.0,
        StatusCode::OK
    );
}

#[sqlx::test]
async fn authorization_matrix(pool: PgPool) {
    let (app, v) = setup(pool.clone());
    let a = seed_actors(&app).await;
    let t2 = second_teacher(&app, &a.admin).await;
    let (_, item) = add(&app, &a.teacher, "openai", KEY_A).await;
    let id = item["id"].as_str().unwrap();
    let one = format!("/api/teacher/keys/{id}");
    let test = format!("{one}/test");
    let calls: Vec<(&str, String, Option<Value>)> = vec![
        ("GET", "/api/teacher/keys".into(), None),
        (
            "POST",
            "/api/teacher/keys".into(),
            Some(json!({"provider":"openai","key":KEY_B})),
        ),
        ("PUT", one.clone(), Some(json!({"key": KEY_B}))),
        ("DELETE", one.clone(), None),
        ("POST", test.clone(), None),
        ("GET", "/api/teacher/ai-providers".into(), None),
    ];
    // 未登入 401；學生、名單外帳號、管理者（不是教師）403
    for (m, p, b) in &calls {
        let (s, e) = call(&app, m, p, None, b.clone()).await;
        assert_eq!(
            (s, code(&e)),
            (StatusCode::UNAUTHORIZED, "unauthorized"),
            "{m} {p}"
        );
        for cookie in [&a.student, &a.outsider, &a.admin] {
            let (s, _) = call(&app, m, p, Some(cookie), b.clone()).await;
            assert_eq!(s, StatusCode::FORBIDDEN, "{m} {p}");
        }
    }
    // 其他教師存取別人的 key：與不存在相同的 404，且沒有任何副作用
    for (m, p, b) in calls.iter().filter(|(_, p, _)| p.contains(id)) {
        let (s, e) = call(&app, m, p, Some(&t2), b.clone()).await;
        assert_eq!(
            (s, code(&e)),
            (StatusCode::NOT_FOUND, "not_found"),
            "{m} {p}"
        );
    }
    assert!(v.calls.lock().unwrap().is_empty());
    let (_, mine) = call(&app, "GET", "/api/teacher/keys", Some(&a.teacher), None).await;
    assert_eq!(mine.as_array().unwrap().len(), 1);
    assert_eq!(mine[0]["mask"], "…aaaa");
    let (_, theirs) = call(&app, "GET", "/api/teacher/keys", Some(&t2), None).await;
    assert!(theirs.as_array().unwrap().is_empty());
    // 不存在與格式不正確的 id
    for p in [
        "/api/teacher/keys/00000000-0000-0000-0000-000000000000",
        "/api/teacher/keys/not-a-uuid",
    ] {
        assert_eq!(
            call(&app, "DELETE", p, Some(&a.teacher), None).await.0,
            StatusCode::NOT_FOUND
        );
    }
}

#[sqlx::test]
async fn csrf_origin_is_required_for_changes(pool: PgPool) {
    let (app, _) = setup(pool);
    let a = seed_actors(&app).await;
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/teacher/keys")
        .header(axum::http::header::COOKIE, a.teacher.clone())
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            json!({"provider":"openai","key":KEY_A}).to_string(),
        ))
        .unwrap();
    assert_eq!(send(&app, req).await.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn without_an_encryption_key_the_feature_is_unavailable(pool: PgPool) {
    let app = build(pool, false, verifier(KeyCheck::Usable), None);
    let a = seed_actors(&app).await;
    let (s, b) = add(&app, &a.teacher, "openai", KEY_A).await;
    assert_eq!(
        (s, code(&b)),
        (StatusCode::SERVICE_UNAVAILABLE, "teacher_keys_unavailable")
    );
    // 清單不需要解密，仍可看
    assert_eq!(
        call(&app, "GET", "/api/teacher/keys", Some(&a.teacher), None)
            .await
            .0,
        StatusCode::OK
    );
}

// ---- GET /api/teacher/keys/models（F-40 模型清單，只當輸入建議）----

/// 依 key 回固定結果；沒登記的 key 一律取不到清單。
#[derive(Default)]
struct FakeLister {
    by_key: Mutex<std::collections::HashMap<String, Vec<String>>>,
    calls: Mutex<Vec<(String, String)>>,
}

#[async_trait]
impl ModelLister for FakeLister {
    async fn list_models(
        &self,
        provider: &Provider,
        key: &str,
    ) -> Result<Vec<String>, ListUnavailable> {
        self.calls
            .lock()
            .unwrap()
            .push((provider.id.into(), key.into()));
        self.by_key
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .ok_or(ListUnavailable)
    }
}

fn lister_with(entries: &[(&str, &[&str])]) -> Arc<FakeLister> {
    let l = Arc::new(FakeLister::default());
    for (key, models) in entries {
        l.by_key.lock().unwrap().insert(
            (*key).into(),
            models.iter().map(|m| (*m).to_owned()).collect(),
        );
    }
    l
}

fn build_with_lister(
    pool: PgPool,
    with_cipher: bool,
    lister: Arc<FakeLister>,
    limit: Option<usize>,
) -> Router {
    register_fixture_pool(&pool);
    let mut config = test_config();
    if with_cipher {
        config.teacher_key_cipher = Some(Arc::new(KeyCipher::parse(ENC).unwrap()));
        config.classroom_ai_routing = true;
    }
    let mut state = AppState::new(pool, config, Arc::new(FakeIdentity), FakeAi::with(vec![]))
        .with_model_lister(lister);
    if let Some(max) = limit {
        state = state.with_model_list_limit(max, std::time::Duration::from_secs(60));
    }
    app(state)
}

const MODELS: &str = "/api/teacher/keys/models";

#[sqlx::test]
async fn model_list_uses_the_teachers_own_key_for_the_chosen_provider(pool: PgPool) {
    let lister = lister_with(&[(KEY_A, &["gpt-a", "gpt-b"])]);
    let app = build_with_lister(pool, true, lister.clone(), None);
    let a = seed_actors(&app).await;
    add(&app, &a.teacher, "openai", KEY_A).await;
    let (s, body) = call(
        &app,
        "GET",
        &format!("{MODELS}?provider=openai"),
        Some(&a.teacher),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["provider"], "openai");
    assert_eq!(body["status"], "ok");
    assert_eq!(body["models"], json!(["gpt-a", "gpt-b"]));
    assert_eq!(
        *lister.calls.lock().unwrap(),
        vec![("openai".to_owned(), KEY_A.to_owned())]
    );
    // key 不會出現在回應裡
    assert!(!body.to_string().contains(KEY_A));
}

#[sqlx::test]
async fn model_list_without_a_key_for_that_provider_says_so_and_never_calls_out(pool: PgPool) {
    let lister = lister_with(&[(KEY_A, &["x"]), (KEY_B, &["y"])]);
    let app = build_with_lister(pool, true, lister.clone(), None);
    let a = seed_actors(&app).await;
    let t2 = second_teacher(&app, &a.admin).await;
    // 別位教師的 openai key 與自己的另一家供應商 key 都不能拿來用
    add(&app, &t2, "openai", KEY_A).await;
    add(&app, &a.teacher, "anthropic", KEY_B).await;
    let (s, body) = call(
        &app,
        "GET",
        &format!("{MODELS}?provider=openai"),
        Some(&a.teacher),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["status"], "no_key");
    assert_eq!(body["models"], json!([]));
    assert!(lister.calls.lock().unwrap().is_empty());
}

#[sqlx::test]
async fn model_list_that_cannot_be_fetched_is_a_status_not_an_error_and_keeps_key_state(
    pool: PgPool,
) {
    // 沒登記清單的 key：取不到
    let lister = lister_with(&[]);
    let app = build_with_lister(pool, true, lister.clone(), None);
    let a = seed_actors(&app).await;
    add(&app, &a.teacher, "openai", KEY_A).await;
    let (s, body) = call(
        &app,
        "GET",
        &format!("{MODELS}?provider=openai"),
        Some(&a.teacher),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["status"], "unavailable");
    assert_eq!(body["models"], json!([]));
    // 取清單不影響 key 的狀態
    let (_, list) = call(&app, "GET", "/api/teacher/keys", Some(&a.teacher), None).await;
    assert_eq!(list[0]["status"], "ok");
}

#[sqlx::test]
async fn model_list_tries_the_next_key_when_the_first_fails_but_not_more_than_three(pool: PgPool) {
    let lister = lister_with(&[(KEY_B, &["from-b"])]);
    let app = build_with_lister(pool.clone(), true, lister.clone(), None);
    let a = seed_actors(&app).await;
    add(&app, &a.teacher, "openai", KEY_A).await;
    add(&app, &a.teacher, "openai", KEY_B).await;
    let (_, body) = call(
        &app,
        "GET",
        &format!("{MODELS}?provider=openai"),
        Some(&a.teacher),
        None,
    )
    .await;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["models"], json!(["from-b"]));
    assert_eq!(lister.calls.lock().unwrap().len(), 2);

    // 四把都失敗：只試前三把
    let lister = lister_with(&[]);
    let app = build_with_lister(pool, true, lister.clone(), None);
    add(&app, &a.teacher, "anthropic", KEY_A).await;
    add(&app, &a.teacher, "anthropic", KEY_B).await;
    add(&app, &a.teacher, "anthropic", KEY_C).await;
    add(
        &app,
        &a.teacher,
        "anthropic",
        "sk-synthetic-DDDD-1111-2222-3333-dddd",
    )
    .await;
    let (_, body) = call(
        &app,
        "GET",
        &format!("{MODELS}?provider=anthropic"),
        Some(&a.teacher),
        None,
    )
    .await;
    assert_eq!(body["status"], "unavailable");
    assert_eq!(lister.calls.lock().unwrap().len(), 3);
}

#[sqlx::test]
async fn model_list_rejects_an_unknown_or_missing_provider(pool: PgPool) {
    let lister = lister_with(&[]);
    let app = build_with_lister(pool, true, lister.clone(), None);
    let a = seed_actors(&app).await;
    for provider in ["ollama", "OpenAI", ""] {
        let (s, e) = call(
            &app,
            "GET",
            &format!("{MODELS}?provider={provider}"),
            Some(&a.teacher),
            None,
        )
        .await;
        assert_eq!(
            (s, code(&e)),
            (StatusCode::BAD_REQUEST, "invalid_provider"),
            "{provider:?}"
        );
    }
    let (s, _) = call(&app, "GET", MODELS, Some(&a.teacher), None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(lister.calls.lock().unwrap().is_empty());
}

#[sqlx::test]
async fn model_list_is_rate_limited_per_teacher(pool: PgPool) {
    let lister = lister_with(&[(KEY_A, &["m"]), (KEY_B, &["n"])]);
    let app = build_with_lister(pool, true, lister.clone(), Some(2));
    let a = seed_actors(&app).await;
    let t2 = second_teacher(&app, &a.admin).await;
    add(&app, &a.teacher, "openai", KEY_A).await;
    add(&app, &t2, "openai", KEY_B).await;
    let path = format!("{MODELS}?provider=openai");
    for _ in 0..2 {
        assert_eq!(
            call(&app, "GET", &path, Some(&a.teacher), None).await.0,
            StatusCode::OK
        );
    }
    let (s, e) = call(&app, "GET", &path, Some(&a.teacher), None).await;
    assert_eq!(
        (s, code(&e)),
        (StatusCode::TOO_MANY_REQUESTS, "model_list_rate_limited")
    );
    assert_eq!(lister.calls.lock().unwrap().len(), 2);
    // 別的教師不受影響
    assert_eq!(
        call(&app, "GET", &path, Some(&t2), None).await.0,
        StatusCode::OK
    );
}

#[sqlx::test]
async fn model_list_is_for_teachers_only(pool: PgPool) {
    let lister = lister_with(&[(KEY_A, &["m"])]);
    let app = build_with_lister(pool, true, lister.clone(), None);
    let a = seed_actors(&app).await;
    add(&app, &a.teacher, "openai", KEY_A).await;
    let path = format!("{MODELS}?provider=openai");
    let (s, e) = call(&app, "GET", &path, None, None).await;
    assert_eq!((s, code(&e)), (StatusCode::UNAUTHORIZED, "unauthorized"));
    for cookie in [&a.student, &a.outsider, &a.admin] {
        assert_eq!(
            call(&app, "GET", &path, Some(cookie), None).await.0,
            StatusCode::FORBIDDEN
        );
    }
    assert!(lister.calls.lock().unwrap().is_empty());
}

#[sqlx::test]
async fn model_list_without_an_encryption_key_is_unavailable(pool: PgPool) {
    let app = build_with_lister(pool, false, lister_with(&[]), None);
    let a = seed_actors(&app).await;
    let (s, b) = call(
        &app,
        "GET",
        &format!("{MODELS}?provider=openai"),
        Some(&a.teacher),
        None,
    )
    .await;
    assert_eq!(
        (s, code(&b)),
        (StatusCode::SERVICE_UNAVAILABLE, "teacher_keys_unavailable")
    );
}
