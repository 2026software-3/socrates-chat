//! #413（F-30.8）：管理者以學號重設沒有電子郵件的學生密碼（S-01.4、S-08.2 第 1 節）。
//! 全部使用合成資料。

mod common;

use axum::{
    Router,
    body::Body,
    http::{Response, StatusCode},
};
use common::*;
use serde_json::json;
use sqlx::PgPool;

async fn login_account(app: &Router, account: &str, pw: &str) -> Response<Body> {
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

async fn reset_by_student_id(
    app: &Router,
    cookie: Option<&str>,
    student_id: &str,
) -> Response<Body> {
    send(
        app,
        json_req(
            "POST",
            "/api/admin/users/reset-password",
            cookie,
            Some(json!({"student_id": student_id})),
        ),
    )
    .await
}

/// 管理者把一位沒有電子郵件的學生匯入教室，回傳匯入時給的臨時密碼。
async fn import_student_without_email(app: &Router, a: &Actors, student_id: &str) -> String {
    let res = import_roster(app, &a.admin, &a.classroom, &format!("王小明,{student_id}")).await;
    assert_eq!(res.status(), StatusCode::OK);
    json_body(res).await["credentials"][0]["temporary_password"]
        .as_str()
        .unwrap()
        .to_string()
}

#[sqlx::test]
async fn admin_resets_a_student_without_email_by_student_id(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let old = import_student_without_email(&app, &a, "11234567A").await;
    // 學號大小寫不分：小寫也能指定到同一個帳號
    let res = reset_by_student_id(&app, Some(&a.admin), "11234567a").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    let new = body["temporary_password"].as_str().unwrap().to_string();
    assert!(new.len() >= 10);
    assert_ne!(new, old);
    assert!(
        body["email"].is_null(),
        "沒有電子郵件的學生回應的 email 為 null"
    );

    assert_eq!(
        login_account(&app, "11234567A", &old).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let res = login_account(&app, "11234567A", &new).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_body(res).await["must_change_password"], true);
}

#[sqlx::test]
async fn student_reset_clears_sessions_and_the_lockout(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let old = import_student_without_email(&app, &a, "11234567A").await;
    let res = login_account(&app, "11234567A", &old).await;
    assert_eq!(res.status(), StatusCode::OK);
    let cookie = session_cookie(&res).unwrap();

    // 連續輸錯達鎖定門檻（同一登入帳號輸入、同一來源 IP）
    for _ in 0..5 {
        assert_eq!(
            login_account(&app, "11234567A", "wrong-password-xyz")
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_json_error(
        login_account(&app, "11234567A", &old).await,
        StatusCode::TOO_MANY_REQUESTS,
        "too_many_attempts",
    )
    .await;

    let res = reset_by_student_id(&app, Some(&a.admin), "11234567A").await;
    assert_eq!(res.status(), StatusCode::OK);
    let new = json_body(res).await["temporary_password"]
        .as_str()
        .unwrap()
        .to_string();

    // 重設後鎖定解除，舊 session 失效
    assert_eq!(
        send(&app, get("/api/me", Some(&cookie))).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login_account(&app, "11234567A", &new).await.status(),
        StatusCode::OK
    );
}

#[sqlx::test]
async fn unknown_student_id_is_not_found_and_nothing_is_created(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let res = reset_by_student_id(&app, Some(&a.admin), "99999999Z").await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(!json_body(res).await.to_string().contains("99999999Z"));
    let (count,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM users WHERE student_id = '99999999Z'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0, "學號不能憑空建立帳號");
}

#[sqlx::test]
async fn malformed_student_id_is_rejected(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    for bad in ["1234567A", "abcdefgh", "12345678AB", ""] {
        assert_json_error(
            reset_by_student_id(&app, Some(&a.admin), bad).await,
            StatusCode::BAD_REQUEST,
            "invalid_student_id",
        )
        .await;
    }
}

#[sqlx::test]
async fn exactly_one_of_email_or_student_id_is_required(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let both = send(
        &app,
        json_req(
            "POST",
            "/api/admin/users/reset-password",
            Some(&a.admin),
            Some(json!({"email": "student@example.com", "student_id": "10000000A"})),
        ),
    )
    .await;
    assert_json_error(both, StatusCode::BAD_REQUEST, "invalid_target").await;

    let neither = send(
        &app,
        json_req(
            "POST",
            "/api/admin/users/reset-password",
            Some(&a.admin),
            Some(json!({})),
        ),
    )
    .await;
    assert_json_error(neither, StatusCode::BAD_REQUEST, "invalid_target").await;
}

#[sqlx::test]
async fn student_id_reset_is_admin_only(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    for (cookie, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(&a.student), StatusCode::FORBIDDEN),
        (Some(&a.outsider), StatusCode::FORBIDDEN),
        (Some(&a.teacher), StatusCode::FORBIDDEN),
    ] {
        let res = reset_by_student_id(&app, cookie.map(String::as_str), "10000000A").await;
        assert_eq!(res.status(), expected);
    }
}

#[sqlx::test]
async fn student_id_reset_audits_the_internal_id_and_never_the_student_id(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    import_student_without_email(&app, &a, "11234567A").await;
    assert_eq!(
        reset_by_student_id(&app, Some(&a.admin), "11234567A")
            .await
            .status(),
        StatusCode::OK
    );

    let (user_id,): (String,) =
        sqlx::query_as("SELECT id::text FROM users WHERE student_id = '11234567A'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT action, target_email FROM audit_logs WHERE action = 'password_reset'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows, vec![("password_reset".to_string(), user_id)]);

    let (leaked,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM audit_logs
         WHERE target_email LIKE '%11234567%' OR reason LIKE '%11234567%' OR action LIKE '%11234567%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(leaked, 0);
}

#[sqlx::test]
async fn a_student_with_an_email_reset_by_student_id_returns_and_audits_the_email(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    // seed 的學生有學號 10000000A 與信箱 student@example.com
    let res = reset_by_student_id(&app, Some(&a.admin), "10000000a").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    assert_eq!(body["email"], "student@example.com");
    let temp = body["temporary_password"].as_str().unwrap().to_string();

    let targets: Vec<(String,)> =
        sqlx::query_as("SELECT target_email FROM audit_logs WHERE action = 'password_reset'")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(targets, vec![("student@example.com".to_string(),)]);

    // 同一組臨時密碼學號與信箱都能登入
    for account in ["10000000A", "student@example.com"] {
        assert_eq!(
            login_account(&app, account, &temp).await.status(),
            StatusCode::OK,
            "{account}"
        );
    }
}

#[sqlx::test]
async fn a_rejected_non_admin_attempt_leaves_the_password_unchanged(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let temp = import_student_without_email(&app, &a, "11234567A").await;
    for cookie in [None, Some(&a.student), Some(&a.outsider), Some(&a.teacher)] {
        let res = reset_by_student_id(&app, cookie.map(String::as_str), "11234567A").await;
        assert!(matches!(
            res.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ));
    }
    // 原本的臨時密碼仍然有效，也沒有留下任何重設的稽核紀錄
    assert_eq!(
        login_account(&app, "11234567A", &temp).await.status(),
        StatusCode::OK
    );
    let (count,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM audit_logs WHERE action = 'password_reset'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
}
