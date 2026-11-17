//! F-40.8-BE (#603): 教師查看自己 key 與教室的 AI 用量；所有資料皆為合成。
mod common;

use std::sync::Arc;

use axum::{Router, http::StatusCode};
use common::*;
use serde_json::{Value, json};
use socrates_chat_backend::{
    AppState, app,
    teacher_key_crypto::KeyCipher,
    teacher_key_usage::{self as usage},
};
use sqlx::PgPool;
use uuid::Uuid;

const ENC: &str = "k1:MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";
const KEY_A: &str = "sk-synthetic-AAAA-1111-2222-3333-aaaa";
const KEY_B: &str = "sk-synthetic-BBBB-1111-2222-3333-bbbb";

fn build(pool: PgPool) -> Router {
    register_fixture_pool(&pool);
    let mut config = test_config();
    config.teacher_key_cipher = Some(Arc::new(KeyCipher::parse(ENC).unwrap()));
    app(AppState::new(
        pool,
        config,
        Arc::new(FakeIdentity),
        FakeAi::with(vec![]),
    ))
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

async fn add_key(app: &Router, cookie: &str, key: &str) -> Uuid {
    let (s, v) = call(
        app,
        "POST",
        "/api/teacher/keys",
        Some(cookie),
        Some(json!({"provider":"openai","key":key})),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    v["id"].as_str().unwrap().parse().unwrap()
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

struct World {
    app: Router,
    a: Actors,
    key: Uuid,
    classroom: Uuid,
}

async fn world(pool: &PgPool) -> World {
    let app = build(pool.clone());
    let a = seed_actors(&app).await;
    let key = add_key(&app, &a.teacher, KEY_A).await;
    let classroom = a.classroom.parse().unwrap();
    World {
        app,
        a,
        key,
        classroom,
    }
}

#[sqlx::test]
async fn key_usage_is_broken_down_by_provider_and_model_with_unreported_flag(pool: PgPool) {
    let w = world(&pool).await;
    usage::record(&pool, w.key, w.classroom, "openai", "m1", Some((10, 5)))
        .await
        .unwrap();
    usage::record(&pool, w.key, w.classroom, "openai", "m1", None)
        .await
        .unwrap();
    usage::record(&pool, w.key, w.classroom, "openai", "m2", Some((1, 2)))
        .await
        .unwrap();
    let (s, v) = call(
        &w.app,
        "GET",
        &format!("/api/teacher/keys/{}/usage?range=all", w.key),
        Some(&w.a.teacher),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["range"], "all");
    assert_eq!(
        v["totals"],
        json!({"requests":3,"input_tokens":11,"output_tokens":7,"unreported_requests":1})
    );
    assert_eq!(
        v["items"],
        json!([
            {"provider":"openai","model":"m1","requests":2,"input_tokens":10,"output_tokens":5,"unreported_requests":1},
            {"provider":"openai","model":"m2","requests":1,"input_tokens":1,"output_tokens":2,"unreported_requests":0},
        ])
    );
    // 沒有金額欄位
    assert!(!v.to_string().contains("cost"));
}

#[sqlx::test]
async fn classroom_usage_sums_keys_and_fallback_models_are_their_own_rows(pool: PgPool) {
    let w = world(&pool).await;
    let k2 = add_key(&w.app, &w.a.teacher, KEY_B).await;
    usage::record(
        &pool,
        w.key,
        w.classroom,
        "openai",
        "primary",
        Some((10, 5)),
    )
    .await
    .unwrap();
    usage::record(&pool, k2, w.classroom, "openai", "primary", Some((1, 1)))
        .await
        .unwrap();
    usage::record(&pool, k2, w.classroom, "openai", "fallback", Some((2, 2)))
        .await
        .unwrap();
    let (s, v) = call(
        &w.app,
        "GET",
        &format!("/api/classrooms/{}/ai-usage?range=all", w.classroom),
        Some(&w.a.teacher),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        v["totals"],
        json!({"requests":3,"input_tokens":13,"output_tokens":8,"unreported_requests":0})
    );
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["model"], "fallback");
    assert_eq!(
        (items[1]["model"].as_str(), items[1]["requests"].as_i64()),
        (Some("primary"), Some(2))
    );
}

#[sqlx::test]
async fn ranges_filter_and_default_and_invalid_values_are_handled(pool: PgPool) {
    let w = world(&pool).await;
    for (ago, r) in [(0, 1), (3, 10), (20, 100), (100, 1000)] {
        sqlx::query("INSERT INTO teacher_key_usage (key_id, classroom_id, provider, model, day, requests) VALUES ($1,$2,'openai','m',(now() AT TIME ZONE 'Asia/Taipei')::date - $3, $4)")
            .bind(w.key).bind(w.classroom).bind(ago).bind(r).execute(&pool).await.unwrap();
    }
    let total = |range: &str| {
        let app = w.app.clone();
        let path = format!("/api/teacher/keys/{}/usage?range={range}", w.key);
        let cookie = w.a.teacher.clone();
        async move {
            call(&app, "GET", &path, Some(&cookie), None).await.1["totals"]["requests"]
                .as_i64()
                .unwrap()
        }
    };
    assert_eq!(total("today").await, 1);
    assert_eq!(total("7d").await, 11);
    assert_eq!(total("30d").await, 111);
    assert_eq!(total("all").await, 1111);
    // 沒帶 range：預設最近 7 天
    let (_, v) = call(
        &w.app,
        "GET",
        &format!("/api/teacher/keys/{}/usage", w.key),
        Some(&w.a.teacher),
        None,
    )
    .await;
    assert_eq!(
        (v["range"].as_str(), v["totals"]["requests"].as_i64()),
        (Some("7d"), Some(11))
    );
    let (s, e) = call(
        &w.app,
        "GET",
        &format!("/api/teacher/keys/{}/usage?range=yesterday", w.key),
        Some(&w.a.teacher),
        None,
    )
    .await;
    assert_eq!((s, code(&e)), (StatusCode::BAD_REQUEST, "invalid_range"));
}

#[sqlx::test]
async fn deleting_a_key_removes_its_usage_and_replacing_keeps_it(pool: PgPool) {
    let w = world(&pool).await;
    let k2 = add_key(&w.app, &w.a.teacher, KEY_B).await;
    usage::record(&pool, w.key, w.classroom, "openai", "m", Some((10, 5)))
        .await
        .unwrap();
    usage::record(&pool, k2, w.classroom, "openai", "m", Some((1, 1)))
        .await
        .unwrap();
    let class_path = format!("/api/classrooms/{}/ai-usage?range=all", w.classroom);
    // 更換 key：用量延續
    let (s, _) = call(
        &w.app,
        "PUT",
        &format!("/api/teacher/keys/{}", w.key),
        Some(&w.a.teacher),
        Some(json!({"key": "sk-synthetic-ZZZZ-1111-2222-3333-zzzz"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, v) = call(
        &w.app,
        "GET",
        &format!("/api/teacher/keys/{}/usage?range=all", w.key),
        Some(&w.a.teacher),
        None,
    )
    .await;
    assert_eq!(v["totals"]["requests"], 1);
    let (_, v) = call(&w.app, "GET", &class_path, Some(&w.a.teacher), None).await;
    assert_eq!(v["totals"]["requests"], 2);
    // 刪除 key：該 key 用量消失，教室總計扣除
    let (s, _) = call(
        &w.app,
        "DELETE",
        &format!("/api/teacher/keys/{}", w.key),
        Some(&w.a.teacher),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = call(
        &w.app,
        "GET",
        &format!("/api/teacher/keys/{}/usage", w.key),
        Some(&w.a.teacher),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (_, v) = call(&w.app, "GET", &class_path, Some(&w.a.teacher), None).await;
    assert_eq!(
        v["totals"],
        json!({"requests":1,"input_tokens":1,"output_tokens":1,"unreported_requests":0})
    );
}

#[sqlx::test]
async fn empty_usage_is_zero(pool: PgPool) {
    let w = world(&pool).await;
    let (s, v) = call(
        &w.app,
        "GET",
        &format!("/api/classrooms/{}/ai-usage", w.classroom),
        Some(&w.a.teacher),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        v["totals"],
        json!({"requests":0,"input_tokens":0,"output_tokens":0,"unreported_requests":0})
    );
    assert_eq!(v["items"], json!([]));
}

#[sqlx::test]
async fn authorization_matrix(pool: PgPool) {
    let w = world(&pool).await;
    let t2 = second_teacher(&w.app, &w.a.admin).await;
    usage::record(&pool, w.key, w.classroom, "openai", "m", Some((10, 5)))
        .await
        .unwrap();
    let paths = [
        format!("/api/teacher/keys/{}/usage", w.key),
        format!("/api/classrooms/{}/ai-usage", w.classroom),
    ];
    for p in &paths {
        let (s, e) = call(&w.app, "GET", p, None, None).await;
        assert_eq!(
            (s, code(&e)),
            (StatusCode::UNAUTHORIZED, "unauthorized"),
            "{p}"
        );
        // 學生、名單外帳號、管理者（不能看教師 key 的用量；教室用量預設不開放，待確認）
        for cookie in [&w.a.student, &w.a.outsider, &w.a.admin] {
            let (s, _) = call(&w.app, "GET", p, Some(cookie), None).await;
            assert_eq!(s, StatusCode::FORBIDDEN, "{p}");
        }
        // 其他教師：與不存在相同的 404
        let (s, e) = call(&w.app, "GET", p, Some(&t2), None).await;
        assert_eq!((s, code(&e)), (StatusCode::NOT_FOUND, "not_found"), "{p}");
    }
    // 不存在或格式不正確的 id
    for p in [
        "/api/teacher/keys/00000000-0000-0000-0000-000000000000/usage",
        "/api/teacher/keys/not-a-uuid/usage",
        "/api/classrooms/00000000-0000-0000-0000-000000000000/ai-usage",
        "/api/classrooms/not-a-uuid/ai-usage",
    ] {
        assert_eq!(
            call(&w.app, "GET", p, Some(&w.a.teacher), None).await.0,
            StatusCode::NOT_FOUND,
            "{p}"
        );
    }
}

#[sqlx::test]
async fn archived_classroom_usage_is_still_readable(pool: PgPool) {
    let w = world(&pool).await;
    usage::record(&pool, w.key, w.classroom, "openai", "m", Some((10, 5)))
        .await
        .unwrap();
    sqlx::query("UPDATE classrooms SET is_archived = true")
        .execute(&pool)
        .await
        .unwrap();
    let (s, v) = call(
        &w.app,
        "GET",
        &format!("/api/classrooms/{}/ai-usage?range=all", w.classroom),
        Some(&w.a.teacher),
        None,
    )
    .await;
    assert_eq!(
        (s, v["totals"]["requests"].as_i64()),
        (StatusCode::OK, Some(1))
    );
}
