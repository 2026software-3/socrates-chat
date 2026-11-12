//! 個人設定更改密碼（F-37、S-18）：所有有內建密碼的帳號都能主動更改；示範帳號不能改、
//! 目前密碼錯誤要限流、成功要稽核。全部使用合成資料。

mod common;

use axum::{Router, body::Body, http::Response, http::StatusCode};
use common::*;
use serde_json::{Value, json};
use socrates_chat_backend::{auth_store, config::Config, demo, password};
use sqlx::PgPool;

/// 每次執行都產生新的密碼，不在原始碼寫死密碼值。
static OLD: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| format!("Old-{}", uuid::Uuid::new_v4()));
static NEW: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| format!("New-{}", uuid::Uuid::new_v4()));

fn config_with_second_admin() -> Config {
    Config {
        admin_emails: vec![
            "admin@example.com".to_string(),
            "admin2@example.com".to_string(),
        ],
        ..test_config()
    }
}

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

async fn login_cookie(app: &Router, account: &str, pw: &str) -> String {
    let res = login(app, account, pw).await;
    assert_eq!(res.status(), StatusCode::OK, "{account}");
    session_cookie(&res).unwrap()
}

async fn change(app: &Router, cookie: &str, current: &str, new: &str) -> Response<Body> {
    send(
        app,
        json_req(
            "POST",
            "/api/auth/change-password",
            Some(cookie),
            Some(json!({"current_password": current, "new_password": new})),
        ),
    )
    .await
}

async fn me(app: &Router, cookie: &str) -> Value {
    let res = send(app, get("/api/me", Some(cookie))).await;
    assert_eq!(res.status(), StatusCode::OK);
    json_body(res).await
}

/// 直接建立一個已有自己密碼（非臨時）的帳號，回傳使用者 id。
async fn user_with_password(pool: &PgPool, email: &str, pw: &str) -> uuid::Uuid {
    let id = seed_user(pool, email).await;
    auth_store::set_password(pool, id, &password::hash(pw).unwrap(), false)
        .await
        .unwrap();
    id
}

/// 管理者替某信箱產生臨時密碼（強制改密碼流程用）。
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

/// 稽核紀錄裡所有 `change_password` 的（操作者, 對象, 其餘欄位串接）。
async fn change_audits(pool: &PgPool) -> Vec<(uuid::Uuid, String, String)> {
    sqlx::query_as(
        "SELECT actor_id, target_email,
                concat_ws('|', action, target_email, coalesce(reason, ''), coalesce(item_count::text, ''))
         FROM audit_logs WHERE action = 'change_password' ORDER BY created_at, id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

// ---- 誰能改、`/api/me` 的 can_change_password ----

#[sqlx::test]
async fn students_teachers_and_administrators_can_each_change_their_own_password(pool: PgPool) {
    let app = test_app_with_config(
        pool.clone(),
        FakeAi::with(vec![]),
        config_with_second_admin(),
    );
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;

    user_with_password(&pool, "student@example.com", &OLD).await;
    user_with_password(&pool, "teacher@example.com", &OLD).await;
    sqlx::query("INSERT INTO teachers (email) VALUES ('teacher@example.com')")
        .execute(&pool)
        .await
        .unwrap();
    // 第二位管理者：由第一位管理者建立帳號，再改成自己的密碼
    let temp = reset(&app, &admin, "admin2@example.com").await;
    let c = login_cookie(&app, "admin2@example.com", &temp).await;
    assert_eq!(
        change(&app, &c, &temp, &OLD).await.status(),
        StatusCode::NO_CONTENT
    );

    for email in [
        "student@example.com",
        "teacher@example.com",
        "admin2@example.com",
    ] {
        let cookie = login_cookie(&app, email, &OLD).await;
        assert_eq!(
            me(&app, &cookie).await["can_change_password"],
            true,
            "{email}"
        );
        assert_eq!(
            change(&app, &cookie, &OLD, &NEW).await.status(),
            StatusCode::NO_CONTENT,
            "{email}"
        );
        assert_eq!(
            login(&app, email, &OLD).await.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(login(&app, email, &NEW).await.status(), StatusCode::OK);
    }
}

#[sqlx::test]
async fn an_account_without_a_built_in_password_cannot_change_one(pool: PgPool) {
    let app = test_app(pool);
    // 只用 Google 登入：沒有內建密碼
    let cookie = login_as(&app, "sub-teacher", "teacher@example.com").await;
    assert_eq!(me(&app, &cookie).await["can_change_password"], false);
    assert_json_error(
        change(&app, &cookie, &OLD, &NEW).await,
        StatusCode::BAD_REQUEST,
        "invalid_current_password",
    )
    .await;
}

#[sqlx::test]
async fn changing_requires_a_login(pool: PgPool) {
    let app = test_app(pool.clone());
    user_with_password(&pool, "student@example.com", &OLD).await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/auth/change-password",
            None,
            Some(json!({"current_password": OLD.as_str(), "new_password": NEW.as_str()})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    // 密碼沒變
    assert_eq!(
        login(&app, "student@example.com", &OLD).await.status(),
        StatusCode::OK
    );
}

#[sqlx::test]
async fn invalid_new_passwords_are_rejected_without_changing_anything(pool: PgPool) {
    let app = test_app(pool.clone());
    user_with_password(&pool, "student@example.com", &OLD).await;
    let cookie = login_cookie(&app, "student@example.com", &OLD).await;
    assert_json_error(
        change(&app, &cookie, &OLD, "short7!").await,
        StatusCode::BAD_REQUEST,
        "invalid_new_password",
    )
    .await;
    assert_json_error(
        change(&app, &cookie, &OLD, &"x".repeat(129)).await,
        StatusCode::BAD_REQUEST,
        "invalid_new_password",
    )
    .await;
    assert_json_error(
        change(&app, &cookie, &OLD, &OLD).await,
        StatusCode::BAD_REQUEST,
        "password_unchanged",
    )
    .await;
    assert_eq!(
        login(&app, "student@example.com", &OLD).await.status(),
        StatusCode::OK
    );
}

#[sqlx::test]
async fn a_successful_change_keeps_this_session_and_signs_out_the_others(pool: PgPool) {
    let app = test_app(pool.clone());
    user_with_password(&pool, "student@example.com", &OLD).await;
    let current = login_cookie(&app, "student@example.com", &OLD).await;
    let other = login_cookie(&app, "student@example.com", &OLD).await;
    assert_eq!(
        change(&app, &current, &OLD, &NEW).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(&app, get("/api/me", Some(&current))).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        send(&app, get("/api/me", Some(&other))).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

// ---- 示範帳號（S-18：公開的固定密碼不能被改掉）----

#[sqlx::test]
async fn demo_accounts_cannot_change_their_public_password_in_demo_mode(pool: PgPool) {
    demo::seed_accounts(&pool).await.unwrap();
    let app = test_app_with_config(pool.clone(), FakeAi::with(vec![]), demo_config());
    let listed = json_body(send(&app, get("/api/demo/accounts", None)).await).await;
    let accounts = listed["accounts"].as_array().unwrap();
    assert!(accounts.iter().any(|a| a["role"] == "teacher"));
    assert!(accounts.iter().any(|a| a["role"] == "student"));
    for a in accounts {
        let (account, pw) = (
            a["account"].as_str().unwrap(),
            a["password"].as_str().unwrap(),
        );
        let cookie = login_cookie(&app, account, pw).await;
        assert_eq!(
            me(&app, &cookie).await["can_change_password"],
            false,
            "{account}"
        );
        assert_json_error(
            change(&app, &cookie, pw, &NEW).await,
            StatusCode::FORBIDDEN,
            "demo_account",
        )
        .await;
        // 公開密碼照舊可用，新密碼不能登入
        assert_eq!(
            login(&app, account, pw).await.status(),
            StatusCode::OK,
            "{account}"
        );
        assert_eq!(
            login(&app, account, &NEW).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
}

#[sqlx::test]
async fn other_users_can_still_change_their_password_in_demo_mode(pool: PgPool) {
    demo::seed_accounts(&pool).await.unwrap();
    user_with_password(&pool, "student@example.com", &OLD).await;
    let app = test_app_with_config(pool, FakeAi::with(vec![]), demo_config());
    let cookie = login_cookie(&app, "student@example.com", &OLD).await;
    assert_eq!(me(&app, &cookie).await["can_change_password"], true);
    assert_eq!(
        change(&app, &cookie, &OLD, &NEW).await.status(),
        StatusCode::NO_CONTENT
    );
}

#[sqlx::test]
async fn demo_account_names_are_ordinary_accounts_when_demo_mode_is_off(pool: PgPool) {
    demo::seed_accounts(&pool).await.unwrap();
    let app = test_app_with_config(pool.clone(), FakeAi::with(vec![]), demo_config());
    let listed = json_body(send(&app, get("/api/demo/accounts", None)).await).await;
    let first = &listed["accounts"][0];
    let (account, pw) = (
        first["account"].as_str().unwrap(),
        first["password"].as_str().unwrap(),
    );
    // 同一批帳號，換成沒開示範模式的 app：不受示範限制
    let plain = test_app(pool);
    let cookie = login_cookie(&plain, account, pw).await;
    assert_eq!(me(&plain, &cookie).await["can_change_password"], true);
    assert_eq!(
        change(&plain, &cookie, pw, &NEW).await.status(),
        StatusCode::NO_CONTENT
    );
}

// ---- 目前密碼錯誤限流（S-18）----

#[sqlx::test]
async fn five_wrong_current_passwords_lock_changes_for_this_user(pool: PgPool) {
    let app = test_app(pool.clone());
    user_with_password(&pool, "student@example.com", &OLD).await;
    let cookie = login_cookie(&app, "student@example.com", &OLD).await;
    for _ in 0..5 {
        assert_json_error(
            change(&app, &cookie, "wrong-password", &NEW).await,
            StatusCode::BAD_REQUEST,
            "invalid_current_password",
        )
        .await;
    }
    // 鎖定期間，即使目前密碼正確也拒絕，密碼不變
    assert_json_error(
        change(&app, &cookie, &OLD, &NEW).await,
        StatusCode::TOO_MANY_REQUESTS,
        "too_many_attempts",
    )
    .await;
    assert_eq!(
        login(&app, "student@example.com", &NEW).await.status(),
        StatusCode::UNAUTHORIZED
    );
    // 只鎖更改密碼，不影響用目前密碼登入
    assert_eq!(
        login(&app, "student@example.com", &OLD).await.status(),
        StatusCode::OK
    );
}

#[sqlx::test]
async fn the_lock_is_per_user_and_expires_after_fifteen_minutes(pool: PgPool) {
    let app = test_app(pool.clone());
    user_with_password(&pool, "student@example.com", &OLD).await;
    user_with_password(&pool, "other@example.com", &OLD).await;
    let cookie = login_cookie(&app, "student@example.com", &OLD).await;
    let other = login_cookie(&app, "other@example.com", &OLD).await;
    for _ in 0..5 {
        change(&app, &cookie, "wrong-password", &NEW).await;
    }
    assert_eq!(
        change(&app, &cookie, &OLD, &NEW).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    // 別人不受影響
    assert_json_error(
        change(&app, &other, "wrong-password", &NEW).await,
        StatusCode::BAD_REQUEST,
        "invalid_current_password",
    )
    .await;
    // 鎖定到期後恢復
    sqlx::query("UPDATE login_failures SET locked_until = now() - interval '1 minute'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        change(&app, &cookie, &OLD, &NEW).await.status(),
        StatusCode::NO_CONTENT
    );
}

#[sqlx::test]
async fn a_successful_change_clears_the_failure_count(pool: PgPool) {
    let app = test_app(pool.clone());
    user_with_password(&pool, "student@example.com", &OLD).await;
    let cookie = login_cookie(&app, "student@example.com", &OLD).await;
    for _ in 0..4 {
        change(&app, &cookie, "wrong-password", &NEW).await;
    }
    assert_eq!(
        change(&app, &cookie, &OLD, &NEW).await.status(),
        StatusCode::NO_CONTENT
    );
    // 計數已歸零：再錯 4 次仍未鎖定，之後輸入正確的目前密碼（現在是 NEW）可以改
    for _ in 0..4 {
        assert_eq!(
            change(&app, &cookie, "wrong-password", &OLD).await.status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        change(&app, &cookie, &NEW, &OLD).await.status(),
        StatusCode::NO_CONTENT
    );
}

#[sqlx::test]
async fn an_admin_reset_clears_the_change_lock(pool: PgPool) {
    let app = test_app(pool.clone());
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    user_with_password(&pool, "student@example.com", &OLD).await;
    let cookie = login_cookie(&app, "student@example.com", &OLD).await;
    for _ in 0..5 {
        change(&app, &cookie, "wrong-password", &NEW).await;
    }
    assert_eq!(
        change(&app, &cookie, &OLD, &NEW).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    let temp = reset(&app, &admin, "student@example.com").await;
    let fresh = login_cookie(&app, "student@example.com", &temp).await;
    assert_eq!(
        change(&app, &fresh, &temp, &NEW).await.status(),
        StatusCode::NO_CONTENT
    );
}

#[sqlx::test]
async fn a_blocked_change_does_not_count_validation_errors_as_failures(pool: PgPool) {
    // 新密碼不合規在驗證目前密碼之前就被擋下，不應累計到鎖定
    let app = test_app(pool.clone());
    user_with_password(&pool, "student@example.com", &OLD).await;
    let cookie = login_cookie(&app, "student@example.com", &OLD).await;
    for _ in 0..6 {
        assert_eq!(
            change(&app, &cookie, &OLD, "short7!").await.status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        change(&app, &cookie, &OLD, &NEW).await.status(),
        StatusCode::NO_CONTENT
    );
}

// ---- 稽核（S-18：change_password，不記密碼）----

#[sqlx::test]
async fn a_successful_change_is_audited_with_the_account_id_and_no_password(pool: PgPool) {
    let app = test_app(pool.clone());
    let id = user_with_password(&pool, "student@example.com", &OLD).await;
    let cookie = login_cookie(&app, "student@example.com", &OLD).await;
    // 失敗的嘗試不留稽核
    change(&app, &cookie, "wrong-password", &NEW).await;
    change(&app, &cookie, &OLD, "short7!").await;
    assert!(change_audits(&pool).await.is_empty());

    assert_eq!(
        change(&app, &cookie, &OLD, &NEW).await.status(),
        StatusCode::NO_CONTENT
    );
    let rows = change_audits(&pool).await;
    assert_eq!(rows.len(), 1);
    let (actor, target, everything) = &rows[0];
    assert_eq!(*actor, id);
    assert_eq!(target, &id.to_string());
    for secret in [
        OLD.as_str(),
        NEW.as_str(),
        "wrong-password",
        "student@example.com",
    ] {
        assert!(!everything.contains(secret), "稽核紀錄不該含密碼或信箱");
    }
}

#[sqlx::test]
async fn the_forced_change_after_a_temporary_password_is_audited_too(pool: PgPool) {
    let app = test_app(pool.clone());
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    let temp = reset(&app, &admin, "student@example.com").await;
    let cookie = login_cookie(&app, "student@example.com", &temp).await;
    assert_eq!(
        change(&app, &cookie, &temp, &NEW).await.status(),
        StatusCode::NO_CONTENT
    );
    let rows = change_audits(&pool).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].1,
        me(&app, &cookie).await["id"].as_str().unwrap().to_string()
    );
}
