//! 管理者清單與新增管理者（`GET/POST /api/admin/admins`）：授權情境、建立帳號、稽核。
//! 全部使用合成資料。

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

async fn list(app: &axum::Router, cookie: Option<&str>) -> (StatusCode, Value) {
    let res = send(app, get("/api/admin/admins", cookie)).await;
    (res.status(), json_body(res).await)
}

async fn add(app: &axum::Router, cookie: Option<&str>, email: &str) -> (StatusCode, Value) {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/admin/admins",
            cookie,
            Some(json!({"email": email})),
        ),
    )
    .await;
    (res.status(), json_body(res).await)
}

async fn audit_actions(pool: &PgPool) -> Vec<(String, String)> {
    sqlx::query_as("SELECT action, target_email FROM audit_logs WHERE action = 'admin_added'")
        .fetch_all(pool)
        .await
        .unwrap()
}

// ---- 授權：未登入、非管理者、管理者 ----

#[sqlx::test]
async fn list_requires_login(pool: PgPool) {
    let app = test_app(pool);
    let (status, _) = list(&app, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn list_is_forbidden_for_non_admins(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    for cookie in [&a.teacher, &a.student, &a.outsider] {
        let (status, _) = list(&app, Some(cookie)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}

#[sqlx::test]
async fn admin_sees_admin_emails_only(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let (status, body) = list(&app, Some(&a.admin)).await;
    assert_eq!(status, StatusCode::OK);
    // 教師、學生、名單外帳號都不在管理者清單內
    assert_eq!(body["emails"], json!(["admin@example.com"]));
}

#[sqlx::test]
async fn add_requires_login_and_leaves_no_audit(pool: PgPool) {
    let app = test_app(pool.clone());
    let (status, _) = add(&app, None, "new-admin@example.com").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(audit_actions(&pool).await.is_empty());
}

#[sqlx::test]
async fn add_is_forbidden_for_non_admins(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    for cookie in [&a.teacher, &a.student, &a.outsider] {
        let (status, _) = add(&app, Some(cookie), "new-admin@example.com").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    let (_, body) = list(&app, Some(&a.admin)).await;
    assert_eq!(body["emails"], json!(["admin@example.com"]));
    assert!(audit_actions(&pool).await.is_empty());
}

// ---- 管理者新增管理者 ----

#[sqlx::test]
async fn admin_adds_new_admin_with_temporary_password(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (status, body) = add(&app, Some(&a.admin), " New-Admin@Example.com ").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["email"], "new-admin@example.com");
    let temp = body["temporary_password"].as_str().unwrap().to_string();

    let (_, listed) = list(&app, Some(&a.admin)).await;
    assert_eq!(
        listed["emails"],
        json!(["admin@example.com", "new-admin@example.com"])
    );

    // 臨時密碼可登入，登入後是管理者，且必須先改密碼
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/auth/login",
            None,
            Some(json!({"email": "new-admin@example.com", "password": temp})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let must_change: bool = sqlx::query_scalar(
        "SELECT must_change_password FROM users WHERE email = 'new-admin@example.com'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(must_change);

    assert_eq!(
        audit_actions(&pool).await,
        vec![(
            "admin_added".to_string(),
            "new-admin@example.com".to_string()
        )]
    );
}

#[sqlx::test]
async fn temporary_password_is_not_stored_in_audit_log(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (_, body) = add(&app, Some(&a.admin), "new-admin@example.com").await;
    let temp = body["temporary_password"].as_str().unwrap();
    let leaked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE target_email LIKE '%' || $1 || '%'",
    )
    .bind(temp)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(leaked, 0);
}

#[sqlx::test]
async fn existing_admin_is_a_noop(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (status, body) = add(&app, Some(&a.admin), "admin@example.com").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["temporary_password"].is_null());
    assert!(audit_actions(&pool).await.is_empty());
}

#[sqlx::test]
async fn existing_non_student_account_is_promoted_without_password_change(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    // 名單外帳號（Google 登入建立）升為管理者：不產生也不更動密碼
    let (status, body) = add(&app, Some(&a.admin), "outsider@example.com").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["temporary_password"].is_null());
    let (_, listed) = list(&app, Some(&a.admin)).await;
    assert_eq!(
        listed["emails"],
        json!(["admin@example.com", "outsider@example.com"])
    );
    // 原本的 session 立即擁有管理者權限
    let (status, _) = list(&app, Some(&a.outsider)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(audit_actions(&pool).await.len(), 1);
}

#[sqlx::test]
async fn student_accounts_cannot_become_admins(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    // 以學號建立的學生帳號（有電子郵件綁定）不能升為管理者
    sqlx::query("UPDATE users SET student_id = '10000000A' WHERE email = 'student@example.com'")
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = add(&app, Some(&a.admin), "student@example.com").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "is_student");
    assert!(audit_actions(&pool).await.is_empty());
}

#[sqlx::test]
async fn invalid_email_is_rejected(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (status, body) = add(&app, Some(&a.admin), "not-an-email").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_email");
}

#[sqlx::test]
async fn admin_can_reset_another_admins_password(pool: PgPool) {
    // 這是原本的缺口：管理者忘記密碼時，另一位管理者要能在清單上找到並重設
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    add(&app, Some(&a.admin), "new-admin@example.com").await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/users/reset-password",
            Some(&a.admin),
            Some(json!({"email": "new-admin@example.com"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let (_, listed) = list(&app, Some(&a.admin)).await;
    assert!(
        listed["emails"]
            .as_array()
            .unwrap()
            .contains(&json!("new-admin@example.com"))
    );
}
