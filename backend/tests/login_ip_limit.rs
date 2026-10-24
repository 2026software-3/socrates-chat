//! 內建登入依來源 IP 的失敗限流（S-08.2、#295）：同一個 IP 不論信箱累計失敗達上限就鎖定，
//! 不影響其他 IP；來源 IP 只在對端是受信任的反向代理時才讀 `X-Forwarded-For`。全部使用合成資料。

mod common;

use std::net::SocketAddr;

use axum::{body::Body, extract::ConnectInfo, http::StatusCode};
use common::*;
use serde_json::json;
use socrates_chat_backend::{auth_store::MAX_IP_LOGIN_FAILURES, client_ip::parse_cidrs};
use sqlx::PgPool;

/// 從指定的 TCP 對端（以及可選的 `X-Forwarded-For`）送出登入請求；`peer` 為 `None` 表示沒有連線資訊。
async fn login_from(
    app: &axum::Router,
    peer: Option<&str>,
    forwarded_for: Option<&str>,
    email: &str,
    pw: &str,
) -> axum::http::Response<Body> {
    let mut req = json_req(
        "POST",
        "/api/auth/login",
        None,
        Some(json!({"email": email, "password": pw})),
    );
    if let Some(peer) = peer {
        let addr = SocketAddr::new(peer.parse().unwrap(), 40_000);
        req.extensions_mut().insert(ConnectInfo(addr));
    }
    if let Some(xff) = forwarded_for {
        req.headers_mut()
            .insert("x-forwarded-for", xff.parse().unwrap());
    }
    send(app, req).await
}

/// 管理者替某信箱建立帳號，回傳臨時密碼（可直接用來登入）。
async fn account(app: &axum::Router, email: &str) -> String {
    let admin = login_as(app, "sub-admin", "admin@example.com").await;
    let res = send(
        app,
        json_req(
            "POST",
            "/api/admin/users/reset-password",
            Some(&admin),
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

/// 用不同的（不存在的）信箱失敗 `n` 次，每次都應回 401。
async fn fail_n_times(app: &axum::Router, peer: &str, xff: Option<&str>, n: i32, tag: &str) {
    for i in 0..n {
        let res = login_from(
            app,
            Some(peer),
            xff,
            &format!("{tag}{i}@example.com"),
            "nope-nope",
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "failure #{i}");
    }
}

fn config_trusting(cidrs: &str) -> socrates_chat_backend::config::Config {
    let mut config = test_config();
    config.trusted_proxies = parse_cidrs(cidrs).unwrap();
    config
}

#[sqlx::test]
async fn one_ip_failing_across_many_emails_gets_locked(pool: PgPool) {
    let app = test_app(pool);
    fail_n_times(&app, "198.51.100.7", None, MAX_IP_LOGIN_FAILURES, "victim").await;

    // 達上限後，換一個從沒失敗過的信箱也被擋
    let res = login_from(
        &app,
        Some("198.51.100.7"),
        None,
        "fresh@example.com",
        "nope-nope",
    )
    .await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(json_body(res).await["error"]["code"], "too_many_attempts");
}

#[sqlx::test]
async fn a_locked_ip_is_rejected_even_with_the_right_password(pool: PgPool) {
    let app = test_app(pool);
    let temp = account(&app, "student@example.com").await;
    fail_n_times(&app, "198.51.100.7", None, MAX_IP_LOGIN_FAILURES, "x").await;

    let res = login_from(
        &app,
        Some("198.51.100.7"),
        None,
        "student@example.com",
        &temp,
    )
    .await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[sqlx::test]
async fn another_ip_is_not_affected_by_a_locked_ip(pool: PgPool) {
    let app = test_app(pool);
    let temp = account(&app, "student@example.com").await;
    fail_n_times(&app, "198.51.100.7", None, MAX_IP_LOGIN_FAILURES, "x").await;

    // 真正的使用者從別處登入不受影響
    let res = login_from(
        &app,
        Some("203.0.113.20"),
        None,
        "student@example.com",
        &temp,
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[sqlx::test]
async fn the_ip_lock_expires(pool: PgPool) {
    let app = test_app(pool.clone());
    fail_n_times(&app, "198.51.100.7", None, MAX_IP_LOGIN_FAILURES, "x").await;
    sqlx::query("UPDATE login_ip_failures SET locked_until = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();

    let res = login_from(
        &app,
        Some("198.51.100.7"),
        None,
        "x@example.com",
        "nope-nope",
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn a_successful_login_does_not_reset_the_ip_counter(pool: PgPool) {
    let app = test_app(pool);
    let temp = account(&app, "student@example.com").await;
    // 攻擊者手上有一個合法帳號：成功登入不能拿來重置自己的失敗額度
    fail_n_times(&app, "198.51.100.7", None, MAX_IP_LOGIN_FAILURES - 1, "x").await;
    let ok = login_from(
        &app,
        Some("198.51.100.7"),
        None,
        "student@example.com",
        &temp,
    )
    .await;
    assert_eq!(ok.status(), StatusCode::OK);
    let last = login_from(
        &app,
        Some("198.51.100.7"),
        None,
        "last@example.com",
        "nope-nope",
    )
    .await;
    assert_eq!(last.status(), StatusCode::UNAUTHORIZED);

    let res = login_from(
        &app,
        Some("198.51.100.7"),
        None,
        "student@example.com",
        &temp,
    )
    .await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[sqlx::test]
async fn requests_without_ip_information_are_not_ip_limited(pool: PgPool) {
    let app = test_app(pool);
    for i in 0..=MAX_IP_LOGIN_FAILURES {
        let res = login_from(&app, None, None, &format!("x{i}@example.com"), "nope-nope").await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "attempt #{i}");
    }
}

#[sqlx::test]
async fn forged_forwarded_for_from_an_untrusted_peer_does_not_dodge_the_limit(pool: PgPool) {
    // 沒有設定受信任代理：標頭一律忽略，每次換一個假 IP 也逃不掉
    let app = test_app(pool);
    for i in 0..MAX_IP_LOGIN_FAILURES {
        let xff = format!("10.9.9.{i}");
        let res = login_from(
            &app,
            Some("198.51.100.7"),
            Some(&xff),
            &format!("x{i}@example.com"),
            "nope-nope",
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
    let res = login_from(
        &app,
        Some("198.51.100.7"),
        Some("10.9.9.99"),
        "x@example.com",
        "nope-nope",
    )
    .await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[sqlx::test]
async fn behind_a_trusted_proxy_each_forwarded_client_has_its_own_budget(pool: PgPool) {
    // 所有請求的 TCP 對端都是代理（127.0.0.1），靠 X-Forwarded-For 區分真正的用戶端
    let app = test_app_with_config(pool, FakeAi::with(vec![]), config_trusting("127.0.0.0/8"));
    fail_n_times(
        &app,
        "127.0.0.1",
        Some("203.0.113.9"),
        MAX_IP_LOGIN_FAILURES,
        "x",
    )
    .await;

    let blocked = login_from(
        &app,
        Some("127.0.0.1"),
        Some("203.0.113.9"),
        "y@example.com",
        "nope-nope",
    )
    .await;
    assert_eq!(blocked.status(), StatusCode::TOO_MANY_REQUESTS);
    // 同一個代理後面的另一個用戶端不受影響（不是所有人共用一個額度）
    let other = login_from(
        &app,
        Some("127.0.0.1"),
        Some("203.0.113.10"),
        "y@example.com",
        "nope-nope",
    )
    .await;
    assert_eq!(other.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn behind_a_trusted_proxy_forged_left_entries_do_not_dodge_the_limit(pool: PgPool) {
    let app = test_app_with_config(pool, FakeAi::with(vec![]), config_trusting("127.0.0.0/8"));
    for i in 0..MAX_IP_LOGIN_FAILURES {
        // 代理在最右邊追加真正的用戶端位址；左邊是用戶端自己塞的假資料
        let xff = format!("10.9.9.{i}, 203.0.113.9");
        let res = login_from(
            &app,
            Some("127.0.0.1"),
            Some(&xff),
            &format!("x{i}@example.com"),
            "nope-nope",
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
    let res = login_from(
        &app,
        Some("127.0.0.1"),
        Some("10.9.9.99, 203.0.113.9"),
        "x@example.com",
        "nope-nope",
    )
    .await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[sqlx::test]
async fn a_direct_client_cannot_borrow_the_proxy_trust(pool: PgPool) {
    // 有設定受信任代理，但請求不是從代理來的（直連者自己填標頭）：用對端位址計
    let app = test_app_with_config(pool, FakeAi::with(vec![]), config_trusting("127.0.0.0/8"));
    for i in 0..MAX_IP_LOGIN_FAILURES {
        let xff = format!("10.9.9.{i}");
        let res = login_from(
            &app,
            Some("198.51.100.7"),
            Some(&xff),
            &format!("x{i}@example.com"),
            "nope-nope",
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
    let res = login_from(
        &app,
        Some("198.51.100.7"),
        Some("10.9.9.99"),
        "x@example.com",
        "nope-nope",
    )
    .await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[sqlx::test]
async fn the_email_lock_still_works_independently(pool: PgPool) {
    // 既有規則不變：同一信箱失敗 5 次鎖定，即使每次來自不同 IP
    let app = test_app(pool);
    for i in 0..5 {
        let peer = format!("198.51.100.{}", 10 + i);
        let res = login_from(&app, Some(&peer), None, "target@example.com", "nope-nope").await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
    let res = login_from(
        &app,
        Some("198.51.100.99"),
        None,
        "target@example.com",
        "nope-nope",
    )
    .await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
}
