//! F-40.9 (#605): 管理者查看教師 key 的設定狀態並刪除；刪除後通知教師並寫稽核。所有資料皆為合成。
mod common;

use std::sync::Arc;

use axum::{Router, http::StatusCode};
use common::*;
use serde_json::{Value, json};
use socrates_chat_backend::{
    AppState, app,
    teacher_key_crypto::KeyCipher,
    teacher_key_usage::{self as usage, Range},
};
use sqlx::PgPool;
use uuid::Uuid;

const ENC: &str = "k1:MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";
const KEY_A: &str = "sk-synthetic-AAAA-1111-2222-3333-aaaa";
const LIST: &str = "/api/admin/teacher-keys";

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

struct World {
    app: Router,
    a: Actors,
    pool: PgPool,
    teacher: Uuid,
    classroom: Uuid,
    key: Uuid,
}

async fn world(pool: PgPool) -> World {
    let app = build(pool.clone());
    let a = seed_actors(&app).await;
    let classroom: Uuid = a.classroom.parse().unwrap();
    let teacher: Uuid = sqlx::query_scalar("SELECT created_by FROM classrooms WHERE id = $1")
        .bind(classroom)
        .fetch_one(&pool)
        .await
        .unwrap();
    let (s, v) = call(
        &app,
        "POST",
        "/api/teacher/keys",
        Some(&a.teacher),
        Some(json!({"provider": "openai", "key": KEY_A})),
    )
    .await;
    assert!(s.is_success(), "{s} {v}");
    let key: Uuid = v["id"].as_str().unwrap().parse().unwrap();
    usage::record(
        &pool,
        key,
        classroom,
        "openai",
        "synthetic-model",
        Some((10, 5)),
    )
    .await
    .unwrap();
    World {
        app,
        a,
        pool,
        teacher,
        classroom,
        key,
    }
}

#[sqlx::test]
async fn only_admins_may_list_or_delete_and_nobody_may_add_or_replace(pool: PgPool) {
    let w = world(pool).await;
    let del = format!("{LIST}/{}", w.key);
    for (cookie, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(w.a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(w.a.teacher.as_str()), StatusCode::FORBIDDEN),
        (Some(w.a.outsider.as_str()), StatusCode::FORBIDDEN),
    ] {
        let (s, _) = call(&w.app, "GET", LIST, cookie, None).await;
        assert_eq!(s, expected, "list as {cookie:?}");
        let (s, _) = call(&w.app, "DELETE", &del, cookie, None).await;
        assert_eq!(s, expected, "delete as {cookie:?}");
    }
    // 沒有被拒絕的請求動到資料
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM teacher_api_keys")
        .fetch_one(&w.pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
    // 管理者也不能新增或更換
    for method in ["POST", "PUT", "PATCH"] {
        let (s, _) = call(
            &w.app,
            method,
            &del,
            Some(&w.a.admin),
            Some(json!({"provider": "openai", "key": "sk-synthetic-NEW"})),
        )
        .await;
        assert_eq!(s, StatusCode::METHOD_NOT_ALLOWED, "{method}");
    }
    let (s, _) = call(
        &w.app,
        "POST",
        LIST,
        Some(&w.a.admin),
        Some(json!({"provider": "openai", "key": "sk-synthetic-NEW"})),
    )
    .await;
    assert_eq!(s, StatusCode::METHOD_NOT_ALLOWED);
}

#[sqlx::test]
async fn admin_sees_configuration_status_but_neither_usage_nor_the_key(pool: PgPool) {
    let w = world(pool).await;
    let (s, v) = call(&w.app, "GET", LIST, Some(&w.a.admin), None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let items = v.as_array().unwrap();
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item["id"], w.key.to_string());
    assert_eq!(item["provider"], "openai");
    assert_eq!(item["provider_name"], "OpenAI");
    assert_eq!(item["status"], "ok");
    assert!(item["mask"].as_str().unwrap().ends_with("aaaa"));
    assert!(item["owner_email"].as_str().unwrap().contains('@'));
    let text = v.to_string();
    assert!(!text.contains(KEY_A) && !text.contains("sk-synthetic"));
    for forbidden in ["usage", "requests", "tokens", "input", "output"] {
        assert!(!text.contains(forbidden), "{forbidden} leaked: {text}");
    }
}

#[sqlx::test]
async fn deleting_removes_the_key_and_its_usage_notifies_the_teacher_and_audits_without_secrets(
    pool: PgPool,
) {
    let w = world(pool).await;
    let before = usage::totals_for_classroom(&w.pool, w.classroom, Range::All)
        .await
        .unwrap();
    assert_eq!(before.requests, 1);

    let (s, _) = call(
        &w.app,
        "DELETE",
        &format!("{LIST}/{}", w.key),
        Some(&w.a.admin),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);

    // key 與用量一併刪除，教室統計扣除
    let keys: i64 = sqlx::query_scalar("SELECT count(*) FROM teacher_api_keys")
        .fetch_one(&w.pool)
        .await
        .unwrap();
    assert_eq!(keys, 0);
    let after = usage::totals_for_classroom(&w.pool, w.classroom, Range::All)
        .await
        .unwrap();
    assert_eq!(after.requests, 0);

    // 教師收到站內通知（只有類型與時間）
    let (s, v) = call(
        &w.app,
        "GET",
        "/api/notifications",
        Some(&w.a.teacher),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let list = v.as_array().unwrap();
    assert_eq!(list.len(), 1, "{v}");
    assert_eq!(list[0]["kind"], "teacher_api_key_deleted");
    assert!(list[0]["created_at"].is_string());
    let mut fields: Vec<_> = list[0].as_object().unwrap().keys().cloned().collect();
    fields.sort();
    assert_eq!(fields, ["created_at", "id", "kind"]);
    // 別人看不到、也不能標記這則通知
    let (s, v) = call(
        &w.app,
        "GET",
        "/api/notifications",
        Some(&w.a.student),
        None,
    )
    .await;
    assert_eq!((s, v.as_array().map(Vec::len)), (StatusCode::OK, Some(0)));
    let id = list[0]["id"].as_str().unwrap();
    let (s, _) = call(
        &w.app,
        "POST",
        &format!("/api/notifications/{id}/read"),
        Some(&w.a.student),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = call(
        &w.app,
        "POST",
        &format!("/api/notifications/{id}/read"),
        Some(&w.a.teacher),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);

    // 稽核：操作者、受影響的教師帳號、動作；不含 key 與用量
    let rows: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(
        "SELECT actor_id, action, target_email FROM audit_logs WHERE action = 'teacher_key_deleted'",
    )
    .fetch_all(&w.pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
    let teacher_email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(w.teacher)
        .fetch_one(&w.pool)
        .await
        .unwrap();
    assert_eq!(rows[0].2.as_deref(), Some(teacher_email.as_str()));
    let dump: String = sqlx::query_scalar(
        "SELECT COALESCE(string_agg(row_to_json(a)::text, ' '), '') FROM audit_logs a WHERE action = 'teacher_key_deleted'",
    )
    .fetch_one(&w.pool)
    .await
    .unwrap();
    let notes: String = sqlx::query_scalar(
        "SELECT COALESCE(string_agg(row_to_json(n)::text, ' '), '') FROM student_notifications n",
    )
    .fetch_one(&w.pool)
    .await
    .unwrap();
    for text in [&dump, &notes] {
        assert!(
            !text.contains("sk-synthetic") && !text.contains("aaaa"),
            "{text}"
        );
    }
}

#[sqlx::test]
async fn deleting_a_missing_key_is_not_found_and_leaves_no_notice_or_audit(pool: PgPool) {
    let w = world(pool).await;
    let path = format!("{LIST}/{}", Uuid::new_v4());
    let (s, _) = call(&w.app, "DELETE", &path, Some(&w.a.admin), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = call(
        &w.app,
        "DELETE",
        &format!("{LIST}/not-a-uuid"),
        Some(&w.a.admin),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let notices: i64 = sqlx::query_scalar("SELECT count(*) FROM student_notifications")
        .fetch_one(&w.pool)
        .await
        .unwrap();
    let audits: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action = 'teacher_key_deleted'")
            .fetch_one(&w.pool)
            .await
            .unwrap();
    assert_eq!((notices, audits), (0, 0));
    // 同一把 key 刪第二次也是 404
    let ok = format!("{LIST}/{}", w.key);
    let (s, _) = call(&w.app, "DELETE", &ok, Some(&w.a.admin), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = call(&w.app, "DELETE", &ok, Some(&w.a.admin), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn existing_student_notifications_still_work_for_students(pool: PgPool) {
    let w = world(pool).await;
    let (s, v) = call(
        &w.app,
        "GET",
        "/api/notifications",
        Some(&w.a.student),
        None,
    )
    .await;
    assert_eq!((s, v.as_array().map(Vec::len)), (StatusCode::OK, Some(0)));
    let (s, _) = call(&w.app, "GET", "/api/notifications", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}
