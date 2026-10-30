//! S-08.2：學生以學號加密碼登入；帳號欄也接受電子郵件，限流以正規化後的登入帳號輸入為單位。

mod common;
use axum::{Router, body::Body, http::Response, http::StatusCode};
use common::*;
use serde_json::json;
use socrates_chat_backend::{auth_store, password};
use sqlx::PgPool;

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

/// 教師匯入一位沒有電子郵件的學生，回傳一次性的臨時密碼。
async fn import_student(app: &Router, a: &Actors, line: &str) -> String {
    let res = send(
        app,
        json_req(
            "POST",
            &format!("/api/classrooms/{}/roster/import", a.classroom),
            Some(&a.teacher),
            Some(json!({"text": line})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    json_body(res).await["credentials"][0]["temporary_password"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn reset(app: &Router, admin: &str, email: &str) -> String {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/admin/users/reset-password",
            Some(admin),
            Some(json!({"email": email})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    json_body(res).await["temporary_password"]
        .as_str()
        .unwrap()
        .to_string()
}

#[sqlx::test]
async fn student_logs_in_with_the_student_id_in_any_case(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let temp = import_student(&app, &a, "王小明,11234567A").await;
    for input in ["11234567A", "11234567a", "  11234567a  "] {
        let res = login(&app, input, &temp).await;
        assert_eq!(res.status(), StatusCode::OK, "{input:?}");
        let cookie = session_cookie(&res).unwrap();
        assert_eq!(json_body(res).await["must_change_password"], true);
        let me = send(&app, get("/api/me", Some(&cookie))).await;
        assert_eq!(me.status(), StatusCode::OK);
    }
}

#[sqlx::test]
async fn email_still_works_in_the_account_field_and_under_the_old_field_name(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let temp = reset(&app, &a.admin, "student@example.com").await;
    assert_eq!(
        login(&app, " STUDENT@example.com ", &temp).await.status(),
        StatusCode::OK
    );
    let old = send(
        &app,
        json_req(
            "POST",
            "/api/auth/login",
            None,
            Some(json!({"email": "student@example.com", "password": temp})),
        ),
    )
    .await;
    assert_eq!(old.status(), StatusCode::OK);
}

#[sqlx::test]
async fn a_student_with_an_email_can_use_either_account(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    // seed 的學生有學號 10000000A 與信箱 student@example.com
    let temp = reset(&app, &a.admin, "student@example.com").await;
    assert_eq!(
        login(&app, "10000000a", &temp).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        login(&app, "student@example.com", &temp).await.status(),
        StatusCode::OK
    );
}

#[sqlx::test]
async fn wrong_unknown_and_malformed_accounts_get_the_same_error(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let temp = import_student(&app, &a, "王小明,11234567A").await;
    assert_json_error(
        login(&app, "11234567A", "wrong-password").await,
        StatusCode::UNAUTHORIZED,
        "invalid_credentials",
    )
    .await;
    for account in [
        "99999999Z",
        "abc",
        "1234567A",
        "",
        "12345678AB",
        "nobody@example.com",
    ] {
        assert_json_error(
            login(&app, account, &temp).await,
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
        )
        .await;
    }
}

#[sqlx::test]
async fn a_disabled_student_is_told_only_after_the_right_password(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let temp = import_student(&app, &a, "王小明,11234567A").await;
    sqlx::query("UPDATE users SET disabled_at = now() WHERE student_id = '11234567A'")
        .execute(&pool)
        .await
        .unwrap();
    assert_json_error(
        login(&app, "11234567A", "wrong-password").await,
        StatusCode::UNAUTHORIZED,
        "invalid_credentials",
    )
    .await;
    assert_json_error(
        login(&app, "11234567A", &temp).await,
        StatusCode::FORBIDDEN,
        "account_disabled",
    )
    .await;
}

#[sqlx::test]
async fn lockout_counts_the_normalized_student_id_and_blocks_the_right_password(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let temp = import_student(&app, &a, "王小明,11234567A").await;
    let other = import_student(&app, &a, "李小華,22345678B").await;
    // 大小寫與空白不同的輸入算同一個登入帳號輸入
    for input in [
        "11234567A",
        "11234567a",
        " 11234567A",
        "11234567a ",
        "11234567A",
    ] {
        assert_json_error(
            login(&app, input, "wrong-password").await,
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
        )
        .await;
    }
    assert_json_error(
        login(&app, "11234567a", &temp).await,
        StatusCode::TOO_MANY_REQUESTS,
        "too_many_attempts",
    )
    .await;
    // 別人的學號不受影響
    assert_eq!(
        login(&app, "22345678B", &other).await.status(),
        StatusCode::OK
    );
}

#[sqlx::test]
async fn failure_records_never_hold_the_student_id_or_email(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    login(&app, "11234567A", "wrong-password").await;
    login(&app, "Student@Example.com", "wrong-password").await;
    let plain: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM login_failures WHERE login_key ILIKE '%11234567A%' OR login_key ILIKE '%example.com%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(plain, 0, "登入帳號輸入只能以雜湊保存");
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM login_failures")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(rows > 0);
    let all_hashed: bool =
        sqlx::query_scalar("SELECT bool_and(length(login_key) = 64) FROM login_failures")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(all_hashed);
    drop(a);
}

#[sqlx::test]
async fn changing_the_password_clears_the_lock_on_both_the_student_id_and_the_email(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let known = "Known-Password-2026";
    let user_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE student_id = '10000000A'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let hash = password::hash(known).unwrap();
    auth_store::set_password(&pool, user_id, &hash, false)
        .await
        .unwrap();
    for account in ["10000000A", "student@example.com"] {
        for _ in 0..5 {
            login(&app, account, "wrong-password").await;
        }
        assert_json_error(
            login(&app, account, known).await,
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_attempts",
        )
        .await;
    }
    auth_store::set_password(&pool, user_id, &hash, false)
        .await
        .unwrap();
    for account in ["10000000A", "student@example.com"] {
        assert_eq!(login(&app, account, known).await.status(), StatusCode::OK);
    }
    drop(a);
}

#[sqlx::test]
async fn login_audit_names_an_email_less_student_by_account_id(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let temp = import_student(&app, &a, "王小明,11234567A").await;
    assert_eq!(
        login(&app, "11234567A", &temp).await.status(),
        StatusCode::OK
    );
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE student_id = '11234567A'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let targets: Vec<String> = sqlx::query_scalar(
        "SELECT target_email FROM audit_logs WHERE action = 'login' AND actor_id = $1",
    )
    .bind(id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(targets, vec![id.to_string()]);
}
