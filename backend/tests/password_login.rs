//! 內建電子郵件＋密碼登入（S-01.1、S-08.2 第 1 節）：不開放註冊、臨時密碼、限流與強制更改密碼。
//! 全部使用合成資料。

mod common;

use axum::{body::Body, http::StatusCode};
use common::*;
use serde_json::{Value, json};
use socrates_chat_backend::{auth_store, password};
use sqlx::PgPool;

const GOOD_PASSWORD: &str = "Correct-Horse-9";

async fn login(app: &axum::Router, email: &str, pw: &str) -> axum::http::Response<Body> {
    send(
        app,
        json_req(
            "POST",
            "/api/auth/login",
            None,
            Some(json!({"email": email, "password": pw})),
        ),
    )
    .await
}

/// 管理者為某信箱產生臨時密碼。
async fn reset(app: &axum::Router, admin: &str, email: &str) -> String {
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

async fn change(app: &axum::Router, cookie: &str, current: &str, new: &str) -> StatusCode {
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
    .status()
}

/// 管理者替某信箱建立帳號，使用者已改好密碼；回傳登入後的 session cookie。
async fn account_with_password(app: &axum::Router, admin: &str, email: &str, pw: &str) -> String {
    let temp = reset(app, admin, email).await;
    let res = login(app, email, &temp).await;
    let cookie = session_cookie(&res).unwrap();
    assert_eq!(
        change(app, &cookie, &temp, pw).await,
        StatusCode::NO_CONTENT
    );
    cookie
}

#[sqlx::test]
async fn admin_reset_creates_account_and_temporary_password_logs_in(pool: PgPool) {
    let app = test_app(pool.clone());
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    let temp = reset(&app, &admin, "Teacher@Example.com").await;
    assert!(temp.len() >= 10);

    let res = login(&app, "teacher@example.com", &temp).await;
    assert_eq!(res.status(), StatusCode::OK);
    let cookie = session_cookie(&res).expect("session cookie");
    let set_cookie = res.headers()[axum::http::header::SET_COOKIE]
        .to_str()
        .unwrap()
        .to_string();
    for attr in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/"] {
        assert!(set_cookie.contains(attr), "missing {attr}");
    }
    assert_eq!(json_body(res).await["must_change_password"], true);

    // 資料庫只存雜湊
    let (hash,): (String,) =
        sqlx::query_as("SELECT password_hash FROM users WHERE email = 'teacher@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(hash.starts_with("$argon2id$"));
    assert!(!hash.contains(&temp));

    let me = send(&app, get("/api/me", Some(&cookie))).await;
    assert_eq!(me.status(), StatusCode::OK);
    let body = json_body(me).await;
    assert_eq!(body["email"], "teacher@example.com");
    assert!(body.get("password_hash").is_none());
}

#[sqlx::test]
async fn unknown_email_and_wrong_password_look_the_same(pool: PgPool) {
    let app = test_app(pool);
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    reset(&app, &admin, "teacher@example.com").await;

    let wrong = login(&app, "teacher@example.com", "not-the-password").await;
    let unknown = login(&app, "nobody@example.com", "not-the-password").await;
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(unknown.status(), StatusCode::UNAUTHORIZED);
    assert!(session_cookie(&wrong).is_none());
    let (a, b) = (json_body(wrong).await, json_body(unknown).await);
    assert_eq!(a["error"]["code"], "invalid_credentials");
    assert_eq!(a["error"]["code"], b["error"]["code"]);
}

#[sqlx::test]
async fn five_failures_lock_the_email_even_for_the_right_password(pool: PgPool) {
    let app = test_app(pool.clone());
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    let temp = reset(&app, &admin, "teacher@example.com").await;

    for _ in 0..5 {
        let res = login(&app, "teacher@example.com", "bad-password").await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
    let res = login(&app, "teacher@example.com", &temp).await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(session_cookie(&res).is_none());
    assert_eq!(json_body(res).await["error"]["code"], "too_many_attempts");

    // 鎖定到期後恢復
    sqlx::query("UPDATE login_failures SET locked_until = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    let res = login(&app, "teacher@example.com", &temp).await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[sqlx::test]
async fn unknown_emails_are_throttled_too(pool: PgPool) {
    let app = test_app(pool);
    for _ in 0..5 {
        login(&app, "ghost@example.com", "bad-password").await;
    }
    let res = login(&app, "ghost@example.com", "bad-password").await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[sqlx::test]
async fn success_clears_failure_count(pool: PgPool) {
    let app = test_app(pool);
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    let temp = reset(&app, &admin, "teacher@example.com").await;
    for _ in 0..4 {
        login(&app, "teacher@example.com", "bad-password").await;
    }
    assert_eq!(
        login(&app, "teacher@example.com", &temp).await.status(),
        StatusCode::OK
    );
    for _ in 0..4 {
        login(&app, "teacher@example.com", "bad-password").await;
    }
    assert_eq!(
        login(&app, "teacher@example.com", &temp).await.status(),
        StatusCode::OK
    );
}

#[sqlx::test]
async fn there_is_no_registration_endpoint(pool: PgPool) {
    let app = test_app(pool.clone());
    for path in ["/api/auth/register", "/api/auth/signup", "/api/register"] {
        let res = send(
            &app,
            json_req(
                "POST",
                path,
                None,
                Some(json!({"email": "new@example.com", "password": GOOD_PASSWORD})),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{path}");
    }
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test]
async fn temporary_password_must_be_changed_before_other_apis(pool: PgPool) {
    let app = test_app(pool);
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    send(
        &app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(&admin),
            Some(json!({"email": "teacher@example.com"})),
        ),
    )
    .await;
    let temp = reset(&app, &admin, "teacher@example.com").await;
    let cookie = session_cookie(&login(&app, "teacher@example.com", &temp).await).unwrap();

    let res = send(&app, get("/api/roster", Some(&cookie))).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        json_body(res).await["error"]["code"],
        "password_change_required"
    );
    // /api/me 仍可呼叫，前端據此導向改密碼頁
    let me = send(&app, get("/api/me", Some(&cookie))).await;
    assert_eq!(me.status(), StatusCode::OK);
    assert_eq!(json_body(me).await["must_change_password"], true);

    assert_eq!(
        change(&app, &cookie, &temp, GOOD_PASSWORD).await,
        StatusCode::NO_CONTENT
    );
    let res = send(&app, get("/api/roster", Some(&cookie))).await;
    assert_eq!(res.status(), StatusCode::OK);
    // 新密碼可登入，舊的臨時密碼不行
    assert_eq!(
        login(&app, "teacher@example.com", GOOD_PASSWORD)
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        login(&app, "teacher@example.com", &temp).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test]
async fn change_password_validates_input(pool: PgPool) {
    let app = test_app(pool);
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    let temp = reset(&app, &admin, "teacher@example.com").await;
    let cookie = session_cookie(&login(&app, "teacher@example.com", &temp).await).unwrap();

    assert_eq!(
        change(&app, &cookie, "wrong-current", GOOD_PASSWORD).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        change(&app, &cookie, &temp, "short7!").await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        change(&app, &cookie, &temp, &temp).await,
        StatusCode::BAD_REQUEST
    );
    // 沒登入
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/auth/change-password",
            None,
            Some(json!({"current_password": temp, "new_password": GOOD_PASSWORD})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    // 沒有任何一次成功：臨時密碼仍有效
    assert_eq!(
        login(&app, "teacher@example.com", &temp).await.status(),
        StatusCode::OK
    );
}

#[sqlx::test]
async fn changing_password_keeps_current_session_and_drops_others(pool: PgPool) {
    let app = test_app(pool);
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    let temp = reset(&app, &admin, "teacher@example.com").await;
    let current = session_cookie(&login(&app, "teacher@example.com", &temp).await).unwrap();
    let other = session_cookie(&login(&app, "teacher@example.com", &temp).await).unwrap();

    assert_eq!(
        change(&app, &current, &temp, GOOD_PASSWORD).await,
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

#[sqlx::test]
async fn admin_reset_invalidates_existing_sessions_and_unlocks(pool: PgPool) {
    let app = test_app(pool);
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    let teacher = account_with_password(&app, &admin, "teacher@example.com", GOOD_PASSWORD).await;
    for _ in 0..5 {
        login(&app, "teacher@example.com", "bad-password").await;
    }

    let temp = reset(&app, &admin, "teacher@example.com").await;
    assert_eq!(
        send(&app, get("/api/me", Some(&teacher))).await.status(),
        StatusCode::UNAUTHORIZED
    );
    // 重設同時解除鎖定
    assert_eq!(
        login(&app, "teacher@example.com", &temp).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        login(&app, "teacher@example.com", GOOD_PASSWORD)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test]
async fn only_admins_can_reset_passwords(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let body = || Some(json!({"email": "student@example.com"}));
    for (cookie, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(&a.student), StatusCode::FORBIDDEN),
        (Some(&a.outsider), StatusCode::FORBIDDEN),
        (Some(&a.teacher), StatusCode::FORBIDDEN),
    ] {
        let res = send(
            &app,
            json_req(
                "POST",
                "/api/admin/users/reset-password",
                cookie.map(String::as_str),
                body(),
            ),
        )
        .await;
        assert_eq!(res.status(), expected);
    }
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/users/reset-password",
            Some(&a.admin),
            Some(json!({"email": "not-an-email"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn roster_import_generates_temporary_passwords_for_new_students(pool: PgPool) {
    let app = test_app(pool);
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/roster/import",
            Some(&admin),
            Some(json!({"text": "email\nS1@example.com\ns2@example.com"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    assert_eq!(body["added"], 2);
    let creds = body["credentials"].as_array().unwrap();
    assert_eq!(creds.len(), 2);
    assert_ne!(
        creds[0]["temporary_password"],
        creds[1]["temporary_password"]
    );

    let s1 = creds
        .iter()
        .find(|c| c["email"] == "s1@example.com")
        .unwrap();
    let temp = s1["temporary_password"].as_str().unwrap();
    let res = login(&app, "s1@example.com", temp).await;
    assert_eq!(res.status(), StatusCode::OK);
    let cookie = session_cookie(&res).unwrap();
    assert_eq!(
        change(&app, &cookie, temp, GOOD_PASSWORD).await,
        StatusCode::NO_CONTENT
    );
    // 名單內學生可以進入學生功能
    let res = send(&app, get("/api/available", Some(&cookie))).await;
    assert_ne!(res.status(), StatusCode::FORBIDDEN);
    assert_ne!(res.status(), StatusCode::UNAUTHORIZED);

    // 再次匯入：已存在的人不會被重新發臨時密碼
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/roster/import",
            Some(&admin),
            Some(json!({"text": "s1@example.com"})),
        ),
    )
    .await;
    let body = json_body(res).await;
    assert_eq!(body["added"], 0);
    assert_eq!(body["credentials"].as_array().unwrap().len(), 0);
    assert_eq!(
        login(&app, "s1@example.com", GOOD_PASSWORD).await.status(),
        StatusCode::OK
    );
}

#[sqlx::test]
async fn roster_import_does_not_overwrite_an_existing_account(pool: PgPool) {
    let app = test_app(pool);
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    login_as(&app, "sub-s", "student@example.com").await; // 已用 Google 登入過、沒有密碼
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/roster/import",
            Some(&admin),
            Some(json!({"text": "student@example.com"})),
        ),
    )
    .await;
    let body = json_body(res).await;
    assert_eq!(body["added"], 1);
    assert_eq!(body["credentials"].as_array().unwrap().len(), 0);
}

#[sqlx::test]
async fn google_login_binds_to_the_password_account_with_the_same_email(pool: PgPool) {
    let app = test_app(pool.clone());
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    let temp = reset(&app, &admin, "student@example.com").await;
    let (before,): (uuid::Uuid,) =
        sqlx::query_as("SELECT id FROM users WHERE email = 'student@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();

    let google = login_as(&app, "sub-s", "Student@Example.com").await;
    let me = json_body(send(&app, get("/api/me", Some(&google))).await).await;
    assert_eq!(me["id"], before.to_string());
    let (sub,): (Option<String>,) = sqlx::query_as("SELECT google_sub FROM users WHERE id = $1")
        .bind(before)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sub.as_deref(), Some("sub-s"));
    // 本人用 Google 登入後，建立者知道的臨時密碼就失效
    assert_eq!(
        login(&app, "student@example.com", &temp).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let (n,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM users WHERE email = 'student@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(n, 1);
}

#[sqlx::test]
async fn google_only_account_cannot_use_password_login(pool: PgPool) {
    let app = test_app(pool);
    login_as(&app, "sub-s", "student@example.com").await;
    let res = login(&app, "student@example.com", "anything-at-all").await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json_body(res).await["error"]["code"], "invalid_credentials");
}

#[sqlx::test]
async fn bootstrap_admins_creates_admin_accounts_once(pool: PgPool) {
    let initial = password::generate_temporary();
    let hash = password::hash(&initial).unwrap();
    let emails = vec!["admin@example.com".to_string()];
    assert_eq!(
        auth_store::bootstrap_admins(&pool, &emails, &hash)
            .await
            .unwrap(),
        1
    );
    // 重啟不會重設已存在的帳號
    assert_eq!(
        auth_store::bootstrap_admins(&pool, &emails, &hash)
            .await
            .unwrap(),
        0
    );

    let app = test_app(pool);
    let res = login(&app, "admin@example.com", &initial).await;
    assert_eq!(res.status(), StatusCode::OK);
    let cookie = session_cookie(&res).unwrap();
    let me: Value = json_body(send(&app, get("/api/me", Some(&cookie))).await).await;
    assert_eq!(me["is_admin"], true);
    assert_eq!(me["must_change_password"], true);
}

#[test]
fn password_hash_round_trips_and_temporary_passwords_are_random() {
    let (p, other) = (
        password::generate_temporary(),
        password::generate_temporary(),
    );
    let h = password::hash(&p).unwrap();
    assert!(password::verify(&p, &h));
    assert!(!password::verify(&other, &h));
    assert!(!password::verify(&p, "not a hash"));
    let (a, b) = (
        password::generate_temporary(),
        password::generate_temporary(),
    );
    assert_ne!(a, b);
    assert_eq!(a.len(), 12);
}

#[sqlx::test]
async fn password_changed_by_the_owner_survives_google_binding(pool: PgPool) {
    let app = test_app(pool);
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    account_with_password(&app, &admin, "student@example.com", GOOD_PASSWORD).await;
    login_as(&app, "sub-s", "student@example.com").await;
    assert_eq!(
        login(&app, "student@example.com", GOOD_PASSWORD)
            .await
            .status(),
        StatusCode::OK
    );
}

#[sqlx::test]
async fn google_login_does_not_inherit_the_must_change_gate(pool: PgPool) {
    let app = test_app(pool);
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    reset(&app, &admin, "student@example.com").await;
    let google = login_as(&app, "sub-s", "student@example.com").await;
    let me = json_body(send(&app, get("/api/me", Some(&google))).await).await;
    assert_eq!(me["must_change_password"], false);
}

#[sqlx::test]
async fn teachers_importing_the_roster_do_not_get_student_credentials(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/roster/import",
            Some(&a.teacher),
            Some(json!({"text": "newstudent@example.com"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    assert_eq!(body["added"], 1);
    assert_eq!(body["credentials"].as_array().unwrap().len(), 0);
    // 沒有建立帳號：教師拿不到可以冒用學生身分的密碼
    let (n,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM users WHERE email = 'newstudent@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(n, 0);
    assert_eq!(
        login(&app, "newstudent@example.com", "anything-at-all")
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test]
async fn roster_import_never_creates_accounts_for_staff_emails(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    // 教師、設定中的管理者信箱就算被放進名單，也不在這裡替他們建帳號
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/roster/import",
            Some(&a.admin),
            Some(json!({"text": "other-teacher@example.com\nadmin2@example.com\nregular@example.com"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    // other-teacher 與 admin2 先登錄為教師／管理者信箱
    sqlx::query("INSERT INTO teachers (email) VALUES ('other-teacher@example.com')")
        .execute(&pool)
        .await
        .unwrap();
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/roster/import",
            Some(&a.admin),
            Some(json!({"text": "other-teacher@example.com\nteacher@example.com\nadmin@example.com"})),
        ),
    )
    .await;
    let body = json_body(res).await;
    assert_eq!(body["credentials"].as_array().unwrap().len(), 0);
}

#[sqlx::test]
async fn pre_created_account_cannot_block_an_admin_email_from_becoming_admin(pool: PgPool) {
    let mut config = test_config();
    config.admin_emails = vec!["admin@example.com".into(), "admin2@example.com".into()];
    let app = test_app_with_config(pool, FakeAi::with(vec![]), config);
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    // 有人搶先替 admin2 的信箱建了帳號
    let temp = reset(&app, &admin, "admin2@example.com").await;
    let google = login_as(&app, "sub-admin2", "admin2@example.com").await;
    let me = json_body(send(&app, get("/api/me", Some(&google))).await).await;
    assert_eq!(me["is_admin"], true);
    // 搶先建立者知道的臨時密碼已失效
    assert_eq!(
        login(&app, "admin2@example.com", &temp).await.status(),
        StatusCode::UNAUTHORIZED
    );
}
