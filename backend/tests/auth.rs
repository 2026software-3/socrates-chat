//! F-01.1 Google 登入與 session（S-08.2）：登入流程、session 與 CSRF 的行為測試。

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use common::*;
use sqlx::PgPool;

#[sqlx::test]
async fn login_redirects_to_google_and_stores_state(pool: PgPool) {
    let app = test_app(pool.clone());
    let res = send(&app, get("/api/auth/google/login", None)).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert!(location(&res).starts_with("https://idp.test/authorize"));
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM oauth_login_states")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[sqlx::test]
async fn callback_creates_session_with_secure_cookie(pool: PgPool) {
    let app = test_app(pool.clone());
    let res = send(&app, get("/api/auth/google/login", None)).await;
    let state = location(&res).split("state=").nth(1).unwrap().to_string();

    let res = send(
        &app,
        get(
            &format!("/api/auth/google/callback?code=ok:sub-1:Alice@Example.com&state={state}"),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&res), APP_URL);
    let set_cookie = res.headers()[header::SET_COOKIE].to_str().unwrap();
    for attr in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/"] {
        assert!(set_cookie.contains(attr), "missing {attr}: {set_cookie}");
    }

    // 資料庫只存雜湊，不存 token 原值
    let token = set_cookie
        .strip_prefix("sid=")
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
    assert!(token.len() >= 43, "256-bit token expected");
    let (email,): (String,) = sqlx::query_as("SELECT email FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(email, "alice@example.com");
}

#[sqlx::test]
async fn callback_with_unknown_state_is_rejected_without_session(pool: PgPool) {
    let app = test_app(pool.clone());
    let res = send(
        &app,
        get(
            "/api/auth/google/callback?code=ok:sub-1:a@example.com&state=nope",
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        location(&res),
        format!("{APP_URL}/login?error=invalid_state")
    );
    assert!(session_cookie(&res).is_none());
}

#[sqlx::test]
async fn state_is_single_use(pool: PgPool) {
    let app = test_app(pool);
    let res = send(&app, get("/api/auth/google/login", None)).await;
    let state = location(&res).split("state=").nth(1).unwrap().to_string();
    let url = format!("/api/auth/google/callback?code=ok:sub-1:a@example.com&state={state}");
    let first = send(&app, get(&url, None)).await;
    assert!(session_cookie(&first).is_some());
    let second = send(&app, get(&url, None)).await;
    assert!(session_cookie(&second).is_none());
    assert_eq!(
        location(&second),
        format!("{APP_URL}/login?error=invalid_state")
    );
}

#[sqlx::test]
async fn unverified_email_is_rejected(pool: PgPool) {
    let app = test_app(pool.clone());
    let res = send(&app, get("/api/auth/google/login", None)).await;
    let state = location(&res).split("state=").nth(1).unwrap().to_string();
    let res = send(
        &app,
        get(
            &format!("/api/auth/google/callback?code=unverified:sub-1:a@example.com&state={state}"),
            None,
        ),
    )
    .await;
    assert_eq!(
        location(&res),
        format!("{APP_URL}/login?error=email_not_verified")
    );
    assert!(session_cookie(&res).is_none());
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test]
async fn failed_token_exchange_is_rejected(pool: PgPool) {
    let app = test_app(pool);
    let res = send(&app, get("/api/auth/google/login", None)).await;
    let state = location(&res).split("state=").nth(1).unwrap().to_string();
    let res = send(
        &app,
        get(
            &format!("/api/auth/google/callback?code=garbage&state={state}"),
            None,
        ),
    )
    .await;
    assert_eq!(
        location(&res),
        format!("{APP_URL}/login?error=login_failed")
    );
    assert!(session_cookie(&res).is_none());
}

#[sqlx::test]
async fn user_cancelling_consent_redirects_with_error(pool: PgPool) {
    let app = test_app(pool);
    let res = send(
        &app,
        get(
            "/api/auth/google/callback?error=access_denied&state=x",
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        location(&res),
        format!("{APP_URL}/login?error=access_denied")
    );
    assert!(session_cookie(&res).is_none());
}

#[sqlx::test]
async fn me_requires_login(pool: PgPool) {
    let app = test_app(pool);
    let res = send(&app, get("/api/me", None)).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let body = json_body(res).await;
    assert_eq!(body["error"]["code"], "unauthorized");
    assert!(body["error"]["request_id"].as_str().unwrap().len() > 8);

    let res = send(&app, get("/api/me", Some("sid=not-a-real-token"))).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn me_returns_profile_for_logged_in_user(pool: PgPool) {
    let app = test_app(pool);
    let cookie = login_as(&app, "sub-1", "alice@example.com").await;
    let res = send(&app, get("/api/me", Some(&cookie))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    assert_eq!(body["email"], "alice@example.com");
    assert_eq!(body["display_name"], "User sub-1");
    assert_eq!(body["is_admin"], false);
}

#[sqlx::test]
async fn admin_email_becomes_admin_on_first_login(pool: PgPool) {
    let app = test_app(pool);
    let cookie = login_as(&app, "sub-a", "Admin@Example.com").await;
    let body = json_body(send(&app, get("/api/me", Some(&cookie))).await).await;
    assert_eq!(body["is_admin"], true);
}

#[sqlx::test]
async fn idle_session_expires_after_seven_days(pool: PgPool) {
    let app = test_app(pool.clone());
    let cookie = login_as(&app, "sub-1", "alice@example.com").await;
    sqlx::query("UPDATE sessions SET last_seen_at = now() - interval '8 days'")
        .execute(&pool)
        .await
        .unwrap();
    let res = send(&app, get("/api/me", Some(&cookie))).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn logout_deletes_session_and_clears_cookie(pool: PgPool) {
    let app = test_app(pool.clone());
    let cookie = login_as(&app, "sub-1", "alice@example.com").await;
    let res = send(
        &app,
        json_req("POST", "/api/auth/logout", Some(&cookie), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let cleared = res.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(cleared.contains("Max-Age=0"));
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    let res = send(&app, get("/api/me", Some(&cookie))).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn cross_site_post_is_rejected(pool: PgPool) {
    let app = test_app(pool.clone());
    let cookie = login_as(&app, "sub-1", "alice@example.com").await;

    let evil = Request::post("/api/auth/logout")
        .header(header::COOKIE, &cookie)
        .header(header::ORIGIN, "https://evil.test")
        .body(Body::empty())
        .unwrap();
    let res = send(&app, evil).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(res).await["error"]["code"], "csrf");

    let no_origin = Request::post("/api/auth/logout")
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&app, no_origin).await.status(), StatusCode::FORBIDDEN);

    // 被拒絕的請求不會登出
    let res = send(&app, get("/api/me", Some(&cookie))).await;
    assert_eq!(res.status(), StatusCode::OK);
}
