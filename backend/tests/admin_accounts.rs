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
    assert_eq!(body["emails"], json!(["admin@example.com"]));
    assert_eq!(body["primary_email"], json!("admin@example.com"));
}

#[sqlx::test]
async fn list_reports_first_env_admin_as_primary_or_null_when_unset(pool: PgPool) {
    let mut cfg = test_config();
    cfg.admin_emails = vec![
        "first@example.com".to_string(),
        "second@example.com".to_string(),
    ];
    let app = test_app_with_config(pool.clone(), FakeAi::with(vec![]), cfg);
    let first = login_as(&app, "sub-first", "first@example.com").await;
    let (status, body) = list(&app, Some(&first)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["primary_email"], json!("first@example.com"));

    let mut empty_cfg = test_config();
    empty_cfg.admin_emails = vec![];
    let app_empty = test_app_with_config(pool, FakeAi::with(vec![]), empty_cfg);
    let (status, body) = list(&app_empty, Some(&first)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["primary_email"].is_null());
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
    let (status, body) = add(&app, Some(&a.admin), "outsider@example.com").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["temporary_password"].is_null());
    let (_, listed) = list(&app, Some(&a.admin)).await;
    assert_eq!(
        listed["emails"],
        json!(["admin@example.com", "outsider@example.com"])
    );
    let (status, _) = list(&app, Some(&a.outsider)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(audit_actions(&pool).await.len(), 1);
}

#[sqlx::test]
async fn student_accounts_cannot_become_admins(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
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

#[sqlx::test]
async fn another_admin_cannot_reset_primary_admins_password(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (status, _) = add(&app, Some(&a.admin), "outsider@example.com").await;
    assert_eq!(status, StatusCode::OK);

    for target in ["admin@example.com", "ADMIN@Example.com"] {
        let res = send(
            &app,
            json_req(
                "POST",
                "/api/admin/users/reset-password",
                Some(&a.outsider),
                Some(json!({"email": target})),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        let body = json_body(res).await;
        assert_eq!(body["error"]["code"], "protected_admin");
    }

    assert_eq!(
        send(&app, get("/api/me", Some(&a.admin))).await.status(),
        StatusCode::OK
    );
    let reset_audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE action = 'password_reset' AND target_email = 'admin@example.com'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reset_audits, 0);
}

#[sqlx::test]
async fn primary_admin_can_reset_own_password(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;

    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/users/reset-password",
            Some(&a.admin),
            Some(json!({"email": "admin@example.com"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    assert_eq!(body["email"], "admin@example.com");
    assert!(body["temporary_password"].as_str().unwrap().len() >= 10);

    let reset_audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE action = 'password_reset' AND target_email = 'admin@example.com'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reset_audits, 1);
}

#[sqlx::test]
async fn only_first_env_admin_is_protected_when_multiple_env_admins_exist(pool: PgPool) {
    let mut cfg = test_config();
    cfg.admin_emails = vec![
        "first@example.com".to_string(),
        "second@example.com".to_string(),
    ];
    let app = test_app_with_config(pool.clone(), FakeAi::with(vec![]), cfg);
    let first = login_as(&app, "sub-first", "first@example.com").await;
    let second = login_as(&app, "sub-second", "second@example.com").await;
    let third = login_as(&app, "sub-third", "third@example.com").await;
    let (status, _) = add(&app, Some(&first), "third@example.com").await;
    assert_eq!(status, StatusCode::OK);

    for actor in [&second, &third] {
        let res = send(
            &app,
            json_req(
                "POST",
                "/api/admin/users/reset-password",
                Some(actor),
                Some(json!({"email": "first@example.com"})),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(json_body(res).await["error"]["code"], "protected_admin");
    }

    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/users/reset-password",
            Some(&third),
            Some(json!({"email": "second@example.com"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/users/reset-password",
            Some(&first),
            Some(json!({"email": "third@example.com"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[sqlx::test]
async fn another_admin_cannot_precreate_primary_admin_via_reset_password(pool: PgPool) {
    let mut cfg = test_config();
    cfg.admin_emails = vec![
        "first@example.com".to_string(),
        "second@example.com".to_string(),
    ];
    let app = test_app_with_config(pool.clone(), FakeAi::with(vec![]), cfg);
    let second = login_as(&app, "sub-second", "second@example.com").await;

    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/users/reset-password",
            Some(&second),
            Some(json!({"email": "first@example.com"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(res).await["error"]["code"], "protected_admin");

    let exists: i64 =
        sqlx::query_scalar("SELECT count(*) FROM users WHERE email = 'first@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(exists, 0);
}
