//! 示範模式（DEMO_MODE）：啟動時建立固定密碼的示範教師與學生，登入頁列出來給發表時一鍵填入。

mod common;
use axum::{Router, body::Body, http::Response, http::StatusCode};
use common::*;
use serde_json::json;
use socrates_chat_backend::{config::Config, demo};
use sqlx::PgPool;

fn demo_config() -> Config {
    Config {
        demo_mode: true,
        ..test_config()
    }
}

async fn login(app: &Router, account: &str, pw: &str) -> Response<Body> {
    send(
        app,
        json_req(
            "POST",
            "/api/auth/login",
            None,
            Some(json!({"account": account, "password": pw})),
        ),
    )
    .await
}

#[sqlx::test]
async fn the_accounts_endpoint_is_hidden_unless_demo_mode_is_on(pool: PgPool) {
    let app = test_app(pool);
    assert_eq!(
        send(&app, get("/api/demo/accounts", None)).await.status(),
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
async fn demo_mode_lists_teacher_and_students_without_any_admin(pool: PgPool) {
    demo::seed_accounts(&pool).await.unwrap();
    let app = test_app_with_config(pool, FakeAi::with(vec![]), demo_config());
    let res = send(&app, get("/api/demo/accounts", None)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    let accounts = body["accounts"].as_array().unwrap();
    assert!(accounts.iter().any(|a| a["role"] == "teacher"));
    assert!(accounts.iter().filter(|a| a["role"] == "student").count() >= 3);
    assert!(accounts.iter().all(|a| a["role"] != "admin"));
}

#[sqlx::test]
async fn every_listed_account_can_log_in_with_the_listed_password(pool: PgPool) {
    demo::seed_accounts(&pool).await.unwrap();
    let app = test_app_with_config(pool, FakeAi::with(vec![]), demo_config());
    let body = json_body(send(&app, get("/api/demo/accounts", None)).await).await;
    for a in body["accounts"].as_array().unwrap() {
        let res = login(
            &app,
            a["account"].as_str().unwrap(),
            a["password"].as_str().unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK, "{}", a["account"]);
        assert_eq!(json_body(res).await["must_change_password"], false);
    }
}

#[sqlx::test]
async fn seeding_twice_resets_a_changed_password_and_never_duplicates(pool: PgPool) {
    demo::seed_accounts(&pool).await.unwrap();
    sqlx::query("UPDATE users SET password_hash = NULL, disabled_at = now() WHERE student_id IS NOT NULL OR email = 'demo.teacher@example.com'")
        .execute(&pool)
        .await
        .unwrap();
    demo::seed_accounts(&pool).await.unwrap();
    let app = test_app_with_config(pool.clone(), FakeAi::with(vec![]), demo_config());
    let body = json_body(send(&app, get("/api/demo/accounts", None)).await).await;
    let first = &body["accounts"][0];
    assert_eq!(
        login(
            &app,
            first["account"].as_str().unwrap(),
            first["password"].as_str().unwrap()
        )
        .await
        .status(),
        StatusCode::OK
    );
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(users, body["accounts"].as_array().unwrap().len() as i64);
}

#[sqlx::test]
async fn seeding_never_touches_an_existing_admin_account(pool: PgPool) {
    sqlx::query("INSERT INTO users (email, password_hash, is_admin) VALUES ('demo.teacher@example.com', 'keep-me', true)")
        .execute(&pool)
        .await
        .unwrap();
    demo::seed_accounts(&pool).await.unwrap();
    let hash: String = sqlx::query_scalar(
        "SELECT password_hash FROM users WHERE email = 'demo.teacher@example.com'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(hash, "keep-me");
}

#[sqlx::test]
async fn the_demo_teacher_acts_as_a_teacher_and_students_have_no_email(pool: PgPool) {
    demo::seed_accounts(&pool).await.unwrap();
    let app = test_app_with_config(pool.clone(), FakeAi::with(vec![]), demo_config());
    let body = json_body(send(&app, get("/api/demo/accounts", None)).await).await;
    let t = body["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["role"] == "teacher")
        .unwrap();
    let res = login(
        &app,
        t["account"].as_str().unwrap(),
        t["password"].as_str().unwrap(),
    )
    .await;
    let cookie = session_cookie(&res).unwrap();
    let me = json_body(send(&app, get("/api/me", Some(&cookie))).await).await;
    assert!(me["roles"].to_string().contains("teacher"));
    let with_email: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM users WHERE student_id IS NOT NULL AND email IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(with_email, 0);
}
