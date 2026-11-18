//! F-40.4-BE (#597): 教室的 AI 對話模型與備援設定；所有資料皆為合成。
mod common;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use axum::{Router, http::StatusCode};
use common::*;
use serde_json::{Value, json};
use socrates_chat_backend::{
    AppState,
    ai_providers::{ModelCheckError, ModelVerifier, Provider},
    app,
    teacher_key_crypto::KeyCipher,
};
use sqlx::PgPool;

const ENC: &str = "k1:MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";
const KEY_OPENAI: &str = "sk-synthetic-AAAA-1111-2222-3333-aaaa";
const KEY_ANTHROPIC: &str = "sk-synthetic-BBBB-1111-2222-3333-bbbb";

#[derive(Default)]
struct FakeModels {
    failures: Mutex<HashMap<(String, String), ModelCheckError>>,
    calls: Mutex<Vec<(String, String, String)>>,
    delay: Mutex<std::time::Duration>,
}

#[async_trait]
impl ModelVerifier for FakeModels {
    async fn verify_model(
        &self,
        provider: &Provider,
        model: &str,
        key: &str,
    ) -> Result<(), ModelCheckError> {
        self.calls
            .lock()
            .unwrap()
            .push((provider.id.into(), model.into(), key.into()));
        let delay = *self.delay.lock().unwrap();
        tokio::time::sleep(delay).await;
        match self
            .failures
            .lock()
            .unwrap()
            .get(&(provider.id.to_string(), model.to_string()))
        {
            Some(e) => Err(*e),
            None => Ok(()),
        }
    }
}

struct World {
    app: Router,
    a: Actors,
    models: Arc<FakeModels>,
}

fn build(pool: PgPool, cipher: bool, limit: Option<usize>) -> (Router, Arc<FakeModels>) {
    build_with(pool, cipher, limit, test_config())
}

fn build_with(
    pool: PgPool,
    cipher: bool,
    limit: Option<usize>,
    mut config: socrates_chat_backend::config::Config,
) -> (Router, Arc<FakeModels>) {
    register_fixture_pool(&pool);
    if cipher {
        config.teacher_key_cipher = Some(Arc::new(KeyCipher::parse(ENC).unwrap()));
        config.classroom_ai_routing = true;
    }
    let models = Arc::new(FakeModels::default());
    let mut state = AppState::new(pool, config, Arc::new(FakeIdentity), FakeAi::with(vec![]))
        .with_model_verifier(models.clone());
    if let Some(n) = limit {
        state = state.with_key_test_limit(n, std::time::Duration::from_secs(600));
    }
    (app(state), models)
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

async fn world(pool: PgPool) -> World {
    let (app, models) = build(pool.clone(), true, None);
    let a = seed_actors(&app).await;
    for (p, k) in [("openai", KEY_OPENAI), ("anthropic", KEY_ANTHROPIC)] {
        let (s, _) = call(
            &app,
            "POST",
            "/api/teacher/keys",
            Some(&a.teacher),
            Some(json!({"provider": p, "key": k})),
        )
        .await;
        assert_eq!(s, StatusCode::CREATED);
    }
    World { app, a, models }
}

fn path(w: &World) -> String {
    format!("/api/classrooms/{}/ai-settings", w.a.classroom)
}

fn put_body(provider: &str, model: &str, fallbacks: Value, confirmed: bool) -> Value {
    json!({"provider": provider, "model": model, "fallbacks": fallbacks, "data_flow_confirmed": confirmed})
}

async fn put(w: &World, body: Value) -> (StatusCode, Value) {
    call(&w.app, "PUT", &path(w), Some(&w.a.teacher), Some(body)).await
}

fn code(v: &Value) -> &str {
    v["error"]["code"].as_str().unwrap_or("")
}

#[sqlx::test]
async fn slow_model_checks_are_not_cut_off_by_the_general_request_timeout(pool: PgPool) {
    // 登入（密碼雜湊）與綁定 key 用一般逾時的 app；session 存在資料庫，換 app 照樣有效
    let seeded = world(pool.clone()).await;
    let mut config = test_config();
    config.request_timeout = std::time::Duration::from_millis(100);
    let (app, models) = build_with(pool, true, None, config);
    *models.delay.lock().unwrap() = std::time::Duration::from_millis(300);
    let w = World {
        app,
        a: seeded.a,
        models,
    };
    let fb =
        json!([{"provider":"anthropic","model":"claude-x"},{"provider":"openai","model":"gpt-b"}]);
    let (s, v) = put(&w, put_body("openai", "gpt-a", fb, true)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(w.models.calls.lock().unwrap().len(), 3);
}

#[sqlx::test]
async fn get_before_any_setting_is_empty(pool: PgPool) {
    let w = world(pool).await;
    let (s, v) = call(&w.app, "GET", &path(&w), Some(&w.a.teacher), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        v,
        json!({"transitional": false, "primary": null, "fallbacks": [], "confirmed_providers": []})
    );
}

#[sqlx::test]
async fn saves_primary_and_ordered_fallbacks_after_verifying_each_with_the_teachers_key(
    pool: PgPool,
) {
    let w = world(pool).await;
    let fb =
        json!([{"provider":"anthropic","model":"claude-x"},{"provider":"openai","model":"gpt-b"}]);
    let (s, v) = put(&w, put_body("openai", "gpt-a", fb.clone(), true)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["primary"], json!({"provider":"openai","model":"gpt-a"}));
    assert_eq!(v["fallbacks"], fb);
    assert_eq!(v["confirmed_providers"], json!(["anthropic", "openai"]));
    let calls = w.models.calls.lock().unwrap().clone();
    assert_eq!(
        calls,
        vec![
            ("openai".into(), "gpt-a".into(), KEY_OPENAI.into()),
            ("anthropic".into(), "claude-x".into(), KEY_ANTHROPIC.into()),
            ("openai".into(), "gpt-b".into(), KEY_OPENAI.into()),
        ]
    );
    let (_, got) = call(&w.app, "GET", &path(&w), Some(&w.a.teacher), None).await;
    assert_eq!(got["primary"], v["primary"]);
    assert_eq!(got["fallbacks"], fb);
    // 回應不含任何 key
    assert!(!format!("{v}{got}").contains("sk-synthetic"));
    // 重新設定：備援整組取代
    let (s, v) = put(&w, put_body("openai", "gpt-c", json!([]), false)).await;
    assert_eq!(s, StatusCode::OK, "已確認過的供應商不必再帶旗標: {v}");
    assert_eq!(v["fallbacks"], json!([]));
}

#[sqlx::test]
async fn first_time_for_a_provider_requires_the_data_flow_confirmation(pool: PgPool) {
    let w = world(pool).await;
    let (s, v) = put(&w, put_body("openai", "gpt-a", json!([]), false)).await;
    assert_eq!(
        (s, code(&v)),
        (StatusCode::BAD_REQUEST, "data_flow_confirmation_required")
    );
    assert!(
        w.models.calls.lock().unwrap().is_empty(),
        "拒絕前不送出任何驗證請求"
    );
    // 缺欄位視同未確認
    let (s, v) = put(
        &w,
        json!({"provider":"openai","model":"gpt-a","fallbacks":[]}),
    )
    .await;
    assert_eq!(
        (s, code(&v)),
        (StatusCode::BAD_REQUEST, "data_flow_confirmation_required")
    );
    let (_, got) = call(&w.app, "GET", &path(&w), Some(&w.a.teacher), None).await;
    assert!(got["primary"].is_null() && got["confirmed_providers"] == json!([]));
    // 新增的備援供應商也算「第一次」
    assert_eq!(
        put(&w, put_body("openai", "gpt-a", json!([]), true))
            .await
            .0,
        StatusCode::OK
    );
    let (s, v) = put(
        &w,
        put_body(
            "openai",
            "gpt-a",
            json!([{"provider":"anthropic","model":"c"}]),
            false,
        ),
    )
    .await;
    assert_eq!(
        (s, code(&v)),
        (StatusCode::BAD_REQUEST, "data_flow_confirmation_required")
    );
}

#[sqlx::test]
async fn teacher_must_hold_a_key_for_every_provider_used(pool: PgPool) {
    let w = world(pool.clone()).await;
    sqlx::query("DELETE FROM teacher_api_keys WHERE provider='anthropic'")
        .execute(&pool)
        .await
        .unwrap();
    let (s, v) = put(&w, put_body("anthropic", "c", json!([]), true)).await;
    assert_eq!(
        (s, code(&v), v["error"]["target"].as_str()),
        (
            StatusCode::BAD_REQUEST,
            "no_key_for_provider",
            Some("primary")
        )
    );
    let (s, v) = put(
        &w,
        put_body(
            "openai",
            "g",
            json!([{"provider":"openai","model":"h"},{"provider":"anthropic","model":"c"}]),
            true,
        ),
    )
    .await;
    assert_eq!(
        (s, code(&v), v["error"]["target"].as_str()),
        (
            StatusCode::BAD_REQUEST,
            "no_key_for_provider",
            Some("fallback_2")
        )
    );
    assert!(w.models.calls.lock().unwrap().is_empty());
    let (_, got) = call(&w.app, "GET", &path(&w), Some(&w.a.teacher), None).await;
    assert!(got["primary"].is_null());
}

#[sqlx::test]
async fn failed_verification_reports_reason_and_target_and_saves_nothing(pool: PgPool) {
    let w = world(pool.clone()).await;
    let reasons = [
        (ModelCheckError::AuthFailed, "model_auth_failed"),
        (ModelCheckError::ModelNotFound, "model_not_found"),
        (
            ModelCheckError::StreamingUnsupported,
            "model_streaming_unsupported",
        ),
        (
            ModelCheckError::StructuredOutputUnsupported,
            "model_structured_output_unsupported",
        ),
        (ModelCheckError::Other, "model_check_failed"),
    ];
    for (err, expected) in reasons {
        w.models
            .failures
            .lock()
            .unwrap()
            .insert(("openai".into(), "bad".into()), err);
        let (s, v) = put(&w, put_body("openai", "bad", json!([]), true)).await;
        assert_eq!(
            (s, code(&v), v["error"]["target"].as_str()),
            (StatusCode::BAD_REQUEST, expected, Some("primary"))
        );
        // 備援失敗指出是第幾個
        let (s, v) = put(&w, put_body("openai", "ok", json!([{"provider":"anthropic","model":"fine"},{"provider":"openai","model":"bad"}]), true)).await;
        assert_eq!(
            (s, code(&v), v["error"]["target"].as_str()),
            (StatusCode::BAD_REQUEST, expected, Some("fallback_2"))
        );
    }
    let (_, got) = call(&w.app, "GET", &path(&w), Some(&w.a.teacher), None).await;
    assert!(got["primary"].is_null());
    assert_eq!(got["confirmed_providers"], json!([]), "失敗不記錄確認");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM classroom_ai_fallbacks")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test]
async fn validates_provider_model_and_fallback_count(pool: PgPool) {
    let w = world(pool).await;
    for provider in ["nope", "OpenAI", ""] {
        let (s, v) = put(&w, put_body(provider, "m", json!([]), true)).await;
        assert_eq!(
            (s, code(&v)),
            (StatusCode::BAD_REQUEST, "invalid_provider"),
            "{provider}"
        );
    }
    let long = "m".repeat(201);
    for model in ["", "   ", long.as_str()] {
        let (s, v) = put(&w, put_body("openai", model, json!([]), true)).await;
        assert_eq!(
            (s, code(&v)),
            (StatusCode::BAD_REQUEST, "invalid_model"),
            "{model:?}"
        );
    }
    let (s, v) = put(
        &w,
        put_body(
            "openai",
            "m",
            json!([{"provider":"nope","model":"x"}]),
            true,
        ),
    )
    .await;
    assert_eq!(
        (s, code(&v), v["error"]["target"].as_str()),
        (
            StatusCode::BAD_REQUEST,
            "invalid_provider",
            Some("fallback_1")
        )
    );
    let four = json!([1, 2, 3, 4].map(|i| json!({"provider":"openai","model":format!("m{i}")})));
    let (s, v) = put(&w, put_body("openai", "m", four, true)).await;
    assert_eq!(
        (s, code(&v)),
        (StatusCode::BAD_REQUEST, "too_many_fallbacks")
    );
    assert!(w.models.calls.lock().unwrap().is_empty());
    // 模型前後空白會去掉
    let (s, v) = put(&w, put_body("openai", "  m  ", json!([]), true)).await;
    assert_eq!(
        (s, v["primary"]["model"].as_str()),
        (StatusCode::OK, Some("m"))
    );
}

#[sqlx::test]
async fn transitional_flag_clears_only_after_a_valid_setting_is_saved(pool: PgPool) {
    let w = world(pool.clone()).await;
    sqlx::query("UPDATE classrooms SET ai_transitional = true")
        .execute(&pool)
        .await
        .unwrap();
    let (_, got) = call(&w.app, "GET", &path(&w), Some(&w.a.teacher), None).await;
    assert_eq!(got["transitional"], true);
    w.models.failures.lock().unwrap().insert(
        ("openai".into(), "bad".into()),
        ModelCheckError::ModelNotFound,
    );
    assert_eq!(
        put(&w, put_body("openai", "bad", json!([]), true)).await.0,
        StatusCode::BAD_REQUEST
    );
    let (_, got) = call(&w.app, "GET", &path(&w), Some(&w.a.teacher), None).await;
    assert_eq!(got["transitional"], true);
    let (s, v) = put(&w, put_body("openai", "good", json!([]), true)).await;
    assert_eq!((s, &v["transitional"]), (StatusCode::OK, &json!(false)));
}

#[sqlx::test]
async fn authorization_matrix(pool: PgPool) {
    let w = world(pool).await;
    let res = send(
        &w.app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(&w.a.admin),
            Some(json!({"email": "t2@example.com"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let t2 = login_as(&w.app, "sub-t2", "t2@example.com").await;
    let body = put_body("openai", "m", json!([]), true);
    for (m, b) in [("GET", None), ("PUT", Some(body.clone()))] {
        let (s, e) = call(&w.app, m, &path(&w), None, b.clone()).await;
        assert_eq!(
            (s, code(&e)),
            (StatusCode::UNAUTHORIZED, "unauthorized"),
            "{m}"
        );
        for cookie in [&w.a.student, &w.a.outsider, &w.a.admin] {
            assert_eq!(
                call(&w.app, m, &path(&w), Some(cookie), b.clone()).await.0,
                StatusCode::FORBIDDEN,
                "{m}"
            );
        }
        let (s, e) = call(&w.app, m, &path(&w), Some(&t2), b.clone()).await;
        assert_eq!((s, code(&e)), (StatusCode::NOT_FOUND, "not_found"), "{m}");
    }
    for p in [
        "/api/classrooms/not-a-uuid/ai-settings",
        "/api/classrooms/00000000-0000-0000-0000-000000000000/ai-settings",
    ] {
        assert_eq!(
            call(&w.app, "GET", p, Some(&w.a.teacher), None).await.0,
            StatusCode::NOT_FOUND
        );
    }
    assert!(
        w.models.calls.lock().unwrap().is_empty(),
        "被拒絕的請求不得觸發供應商呼叫"
    );
    // 其他教師的 key 不能拿來驗證：t2 沒有 key，且根本進不來
    let (_, got) = call(&w.app, "GET", &path(&w), Some(&w.a.teacher), None).await;
    assert!(got["primary"].is_null());
}

#[sqlx::test]
async fn archived_classroom_cannot_be_changed_but_can_be_read(pool: PgPool) {
    let w = world(pool.clone()).await;
    sqlx::query("UPDATE classrooms SET is_archived = true")
        .execute(&pool)
        .await
        .unwrap();
    let (s, v) = put(&w, put_body("openai", "m", json!([]), true)).await;
    assert_eq!((s, code(&v)), (StatusCode::CONFLICT, "classroom_archived"));
    assert_eq!(
        call(&w.app, "GET", &path(&w), Some(&w.a.teacher), None)
            .await
            .0,
        StatusCode::OK
    );
}

#[sqlx::test]
async fn without_an_encryption_key_saving_is_unavailable(pool: PgPool) {
    let (app, _) = build(pool, false, None);
    let a = seed_actors(&app).await;
    let (s, v) = call(
        &app,
        "PUT",
        &format!("/api/classrooms/{}/ai-settings", a.classroom),
        Some(&a.teacher),
        Some(put_body("openai", "m", json!([]), true)),
    )
    .await;
    assert_eq!(
        (s, code(&v)),
        (StatusCode::SERVICE_UNAVAILABLE, "teacher_keys_unavailable")
    );
}

#[sqlx::test]
async fn verification_is_rate_limited_per_teacher(pool: PgPool) {
    let (app, models) = build(pool, true, Some(2));
    let a = seed_actors(&app).await;
    let (s, _) = call(
        &app,
        "POST",
        "/api/teacher/keys",
        Some(&a.teacher),
        Some(json!({"provider":"openai","key":KEY_OPENAI})),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let p = format!("/api/classrooms/{}/ai-settings", a.classroom);
    for _ in 0..2 {
        assert_eq!(
            call(
                &app,
                "PUT",
                &p,
                Some(&a.teacher),
                Some(put_body("openai", "m", json!([]), true))
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    let (s, v) = call(
        &app,
        "PUT",
        &p,
        Some(&a.teacher),
        Some(put_body("openai", "m", json!([]), true)),
    )
    .await;
    assert_eq!(
        (s, code(&v)),
        (StatusCode::TOO_MANY_REQUESTS, "key_test_rate_limited")
    );
    assert_eq!(models.calls.lock().unwrap().len(), 2);
}

#[sqlx::test]
async fn students_see_only_the_provider_name(pool: PgPool) {
    let w = world(pool.clone()).await;
    let student_path = format!("/api/classrooms/{}/ai-provider", w.a.classroom);
    let (s, v) = call(&w.app, "GET", &student_path, Some(&w.a.student), None).await;
    assert_eq!((s, &v), (StatusCode::OK, &json!({"provider": null})));
    put(
        &w,
        put_body(
            "openai",
            "secret-model-id",
            json!([{"provider":"anthropic","model":"fb-model"}]),
            true,
        ),
    )
    .await;
    let (s, v) = call(&w.app, "GET", &student_path, Some(&w.a.student), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v, json!({"provider": {"id": "openai", "name": "OpenAI"}}));
    let text = v.to_string();
    for hidden in ["secret-model-id", "fb-model", "sk-synthetic", "anthropic"] {
        assert!(!text.contains(hidden), "{hidden}");
    }
    // 過渡中沿用系統 key：不顯示供應商
    sqlx::query("UPDATE classrooms SET ai_transitional = true")
        .execute(&pool)
        .await
        .unwrap();
    let (_, v) = call(&w.app, "GET", &student_path, Some(&w.a.student), None).await;
    assert_eq!(v, json!({"provider": null}));
    // 未登入、名單外、教師與管理者
    assert_eq!(
        call(&w.app, "GET", &student_path, None, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    for cookie in [&w.a.outsider, &w.a.teacher, &w.a.admin] {
        assert_eq!(
            call(&w.app, "GET", &student_path, Some(cookie), None)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    // 學生只在另一間教室的名單內：看這間教室與不存在相同 404
    let other: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO classrooms (name, created_by) SELECT 'other', created_by FROM classrooms LIMIT 1 RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE enrollments SET classroom_id = $1")
        .bind(other)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        call(&w.app, "GET", &student_path, Some(&w.a.student), None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}
