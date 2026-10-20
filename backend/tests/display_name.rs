//! 學生自訂顯示名稱（S-08.2 第 3 節）：PATCH /api/me。使用合成資料。

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::{Value, json};
use socrates_chat_backend::auth_store::upsert_user;
use sqlx::PgPool;

async fn rename(app: &axum::Router, cookie: Option<&str>, body: Value) -> (StatusCode, Value) {
    let res = send(app, json_req("PATCH", "/api/me", cookie, Some(body))).await;
    (res.status(), json_body(res).await)
}

async fn me_name(app: &axum::Router, cookie: &str) -> Value {
    let res = send(app, get("/api/me", Some(cookie))).await;
    json_body(res).await["display_name"].clone()
}

#[sqlx::test]
async fn unauthenticated_is_401(pool: PgPool) {
    let app = test_app(pool);
    let (status, _) = rename(&app, None, json!({"display_name": "小明"})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn non_students_are_403(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    for cookie in [&a.teacher, &a.admin, &a.outsider] {
        let (status, _) = rename(&app, Some(cookie), json!({"display_name": "小明"})).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    assert_eq!(me_name(&app, &a.teacher).await, "User sub-teacher");
}

#[sqlx::test]
async fn student_renames_self_and_it_shows_in_me_and_students(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let (status, body) = rename(&app, Some(&a.student), json!({"display_name": "小明"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["display_name"], "小明");
    assert_eq!(me_name(&app, &a.student).await, "小明");

    let res = send(&app, get("/api/students", Some(&a.teacher))).await;
    let list = json_body(res).await;
    let row = list
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["email"] == "student@example.com")
        .unwrap()
        .clone();
    assert_eq!(row["display_name"], "小明");
}

#[sqlx::test]
async fn name_is_trimmed(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let (status, body) = rename(&app, Some(&a.student), json!({"display_name": "  小明 \n"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["display_name"], "小明");
}

#[sqlx::test]
async fn null_and_blank_clear_the_name(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    for cleared in [json!(null), json!(""), json!("   ")] {
        rename(&app, Some(&a.student), json!({"display_name": "小明"})).await;
        let (status, body) = rename(&app, Some(&a.student), json!({"display_name": cleared})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["display_name"], Value::Null);
        assert_eq!(me_name(&app, &a.student).await, Value::Null);
    }
}

#[sqlx::test]
async fn absent_field_keeps_the_name(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let (status, body) = rename(&app, Some(&a.student), json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["display_name"], "User sub-student");
}

#[sqlx::test]
async fn length_limit_is_20_characters_not_bytes(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let exactly_20 = "字".repeat(20);
    let (status, body) = rename(&app, Some(&a.student), json!({"display_name": exactly_20})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["display_name"], exactly_20);

    let (status, body) = rename(
        &app,
        Some(&a.student),
        json!({"display_name": "字".repeat(21)}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_display_name");
    assert_eq!(me_name(&app, &a.student).await, exactly_20);
}

#[sqlx::test]
async fn control_characters_inside_the_name_are_rejected(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    // 換行、tab、NUL、DEL、C1 控制字元（U+0085），以及行／段落分隔字元（U+2028、U+2029，類別 Zl／Zp，
    // 不屬於 is_control 但同樣會破壞排版）；頭尾的空白會先被去掉，不算在內
    for name in [
        "小\n明",
        "小\t明",
        "小\u{0}明",
        "小\u{7f}明",
        "小\u{85}明",
        "小\u{2028}明",
        "小\u{2029}明",
    ] {
        let (status, body) = rename(&app, Some(&a.student), json!({"display_name": name})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name:?}");
        assert_eq!(body["error"]["code"], "invalid_display_name");
    }
    assert_eq!(me_name(&app, &a.student).await, "User sub-student");
}

#[sqlx::test]
async fn other_unusual_characters_are_still_allowed(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let name = "Ünï 小明 😀·-";
    let (status, body) = rename(&app, Some(&a.student), json!({"display_name": name})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["display_name"], name);
}

#[sqlx::test]
async fn length_is_counted_after_trimming(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let padded = format!("  {}  ", "字".repeat(20));
    let (status, _) = rename(&app, Some(&a.student), json!({"display_name": padded})).await;
    assert_eq!(status, StatusCode::OK);
}

#[sqlx::test]
async fn google_login_does_not_overwrite_a_custom_name(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    rename(&app, Some(&a.student), json!({"display_name": "小明"})).await;
    let cookie = login_as(&app, "sub-student", "student@example.com").await;
    assert_eq!(me_name(&app, &cookie).await, "小明");
}

#[sqlx::test]
async fn google_login_fills_the_name_when_it_was_cleared(pool: PgPool) {
    let u = upsert_user(&pool, "sub-1", "a@example.com", Some("Old"), false)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET display_name = NULL WHERE id = $1")
        .bind(u.id)
        .execute(&pool)
        .await
        .unwrap();
    let again = upsert_user(&pool, "sub-1", "a@example.com", Some("New"), false)
        .await
        .unwrap();
    assert_eq!(again.display_name.as_deref(), Some("New"));
}
