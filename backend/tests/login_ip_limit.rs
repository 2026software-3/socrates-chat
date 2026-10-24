//! 內建登入的 IP 維度限流（S-08.2、#295）：失敗鎖定以（信箱, 來源 IP）為單位，別處的失敗不會
//! 鎖住真正的使用者，同一個 IP（例如校園 NAT）下不同信箱也互不影響；另有每個 IP 的請求頻率上限。
//! 來源 IP 只在對端是受信任的反向代理時才讀 `X-Forwarded-For`。全部使用合成資料。

mod common;

use std::net::SocketAddr;

use axum::{body::Body, extract::ConnectInfo, http::StatusCode};
use common::*;
use serde_json::json;
use socrates_chat_backend::{client_ip::parse_cidrs, login_rate_limit::BURST};
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

fn config_trusting(cidrs: &str) -> socrates_chat_backend::config::Config {
    let mut config = test_config();
    config.trusted_proxies = parse_cidrs(cidrs).unwrap();
    config
}

/// 對同一個信箱、同一個來源失敗 `n` 次（密碼錯誤），每次都應回 401。
async fn fail(app: &axum::Router, peer: &str, xff: Option<&str>, email: &str, n: usize) {
    for i in 0..n {
        let res = login_from(app, Some(peer), xff, email, "nope-nope").await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "failure #{i}");
    }
}

fn status(res: &axum::http::Response<Body>) -> StatusCode {
    res.status()
}

#[sqlx::test]
async fn five_failures_from_the_same_ip_lock_that_email_there(pool: PgPool) {
    let app = test_app(pool);
    let temp = account(&app, "student@example.com").await;
    fail(&app, "198.51.100.7", None, "student@example.com", 5).await;

    // 鎖定期間即使密碼正確也被拒，錯誤碼與一般限流相同
    let res = login_from(
        &app,
        Some("198.51.100.7"),
        None,
        "student@example.com",
        &temp,
    )
    .await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(json_body(res).await["error"]["code"], "too_many_attempts");
}

#[sqlx::test]
async fn failures_from_another_ip_do_not_lock_the_real_user_out(pool: PgPool) {
    let app = test_app(pool);
    let temp = account(&app, "admin2@example.com").await;
    // 攻擊者從自己的 IP 亂試別人的信箱
    fail(&app, "198.51.100.7", None, "admin2@example.com", 5).await;
    let locked = login_from(
        &app,
        Some("198.51.100.7"),
        None,
        "admin2@example.com",
        &temp,
    )
    .await;
    assert_eq!(status(&locked), StatusCode::TOO_MANY_REQUESTS);

    // 本人從平常的 IP 以正確密碼登入，不被鎖在門外
    let ok = login_from(
        &app,
        Some("203.0.113.20"),
        None,
        "admin2@example.com",
        &temp,
    )
    .await;
    assert_eq!(status(&ok), StatusCode::OK);
}

#[sqlx::test]
async fn many_emails_failing_from_one_ip_do_not_lock_the_whole_ip(pool: PgPool) {
    // 校園 NAT：同一個 IP 後面很多人，一個人打錯不能連累其他人
    let app = test_app(pool);
    let temp = account(&app, "student@example.com").await;
    for i in 0..25 {
        let res = login_from(
            &app,
            Some("198.51.100.7"),
            None,
            &format!("other{i}@example.com"),
            "nope-nope",
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "email #{i}");
    }
    let ok = login_from(
        &app,
        Some("198.51.100.7"),
        None,
        "student@example.com",
        &temp,
    )
    .await;
    assert_eq!(status(&ok), StatusCode::OK);
}

#[sqlx::test]
async fn the_lock_expires(pool: PgPool) {
    let app = test_app(pool.clone());
    fail(&app, "198.51.100.7", None, "x@example.com", 5).await;
    sqlx::query("UPDATE login_failures SET locked_until = now() - interval '1 second'")
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
async fn success_clears_only_its_own_ip_counter(pool: PgPool) {
    let app = test_app(pool);
    let temp = account(&app, "student@example.com").await;
    fail(&app, "198.51.100.7", None, "student@example.com", 4).await;
    fail(&app, "203.0.113.20", None, "student@example.com", 4).await;

    // 本人從 203.0.113.20 成功登入，只清掉那一組；攻擊者那一組的計數還在
    let ok = login_from(
        &app,
        Some("203.0.113.20"),
        None,
        "student@example.com",
        &temp,
    )
    .await;
    assert_eq!(status(&ok), StatusCode::OK);

    let fifth = login_from(
        &app,
        Some("198.51.100.7"),
        None,
        "student@example.com",
        "nope-nope",
    )
    .await;
    assert_eq!(status(&fifth), StatusCode::UNAUTHORIZED);
    let locked = login_from(
        &app,
        Some("198.51.100.7"),
        None,
        "student@example.com",
        "nope-nope",
    )
    .await;
    assert_eq!(status(&locked), StatusCode::TOO_MANY_REQUESTS);
}

#[sqlx::test]
async fn admin_reset_clears_the_lock_for_every_ip(pool: PgPool) {
    let app = test_app(pool);
    account(&app, "student@example.com").await;
    fail(&app, "198.51.100.7", None, "student@example.com", 5).await;
    fail(&app, "203.0.113.20", None, "student@example.com", 5).await;

    let new_temp = account(&app, "student@example.com").await;
    for peer in ["198.51.100.7", "203.0.113.20"] {
        let res = login_from(&app, Some(peer), None, "student@example.com", &new_temp).await;
        assert_eq!(status(&res), StatusCode::OK, "{peer}");
    }
}

#[sqlx::test]
async fn requests_without_ip_information_share_one_email_lock(pool: PgPool) {
    let app = test_app(pool);
    for i in 0..5 {
        let res = login_from(&app, None, None, "x@example.com", "nope-nope").await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "failure #{i}");
    }
    let res = login_from(&app, None, None, "x@example.com", "nope-nope").await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[sqlx::test]
async fn forged_forwarded_for_from_an_untrusted_peer_does_not_dodge_the_lock(pool: PgPool) {
    // 沒有設定受信任代理：標頭一律忽略，每次換一個假 IP 也逃不掉
    let app = test_app(pool);
    for i in 0..5 {
        let xff = format!("10.9.9.{i}");
        let res = login_from(
            &app,
            Some("198.51.100.7"),
            Some(&xff),
            "x@example.com",
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
async fn a_direct_client_cannot_borrow_the_proxy_trust(pool: PgPool) {
    // 有設定受信任代理，但請求不是從代理來的（直連者自己填標頭）：用對端位址計
    let app = test_app_with_config(pool, FakeAi::with(vec![]), config_trusting("127.0.0.0/8"));
    for i in 0..5 {
        let xff = format!("10.9.9.{i}");
        let res = login_from(
            &app,
            Some("198.51.100.7"),
            Some(&xff),
            "x@example.com",
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
async fn behind_a_trusted_proxy_the_forwarded_client_is_the_lock_unit(pool: PgPool) {
    // 所有請求的 TCP 對端都是代理（127.0.0.1），靠 X-Forwarded-For 區分真正的用戶端
    let app = test_app_with_config(pool, FakeAi::with(vec![]), config_trusting("127.0.0.0/8"));
    let temp = account(&app, "student@example.com").await;
    fail(
        &app,
        "127.0.0.1",
        Some("203.0.113.9"),
        "student@example.com",
        5,
    )
    .await;

    let blocked = login_from(
        &app,
        Some("127.0.0.1"),
        Some("203.0.113.9"),
        "student@example.com",
        &temp,
    )
    .await;
    assert_eq!(status(&blocked), StatusCode::TOO_MANY_REQUESTS);
    // 同一個代理後面的另一個用戶端不受影響（不是所有人共用一組）
    let other = login_from(
        &app,
        Some("127.0.0.1"),
        Some("203.0.113.10"),
        "student@example.com",
        &temp,
    )
    .await;
    assert_eq!(status(&other), StatusCode::OK);
}

#[sqlx::test]
async fn behind_a_trusted_proxy_forged_left_entries_do_not_dodge_the_lock(pool: PgPool) {
    let app = test_app_with_config(pool, FakeAi::with(vec![]), config_trusting("127.0.0.0/8"));
    for i in 0..5 {
        // 代理在最右邊追加真正的用戶端位址；左邊是用戶端自己塞的假資料
        let xff = format!("10.9.9.{i}, 203.0.113.9");
        let res = login_from(
            &app,
            Some("127.0.0.1"),
            Some(&xff),
            "x@example.com",
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

/// 送出一個會立刻被 JSON 驗證擋下的請求（422），用來便宜地消耗請求頻率額度。
async fn cheap_request(app: &axum::Router, peer: &str, xff: Option<&str>) -> StatusCode {
    let mut req = json_req("POST", "/api/auth/login", None, Some(json!({})));
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::new(peer.parse().unwrap(), 40_000)));
    if let Some(xff) = xff {
        req.headers_mut()
            .insert("x-forwarded-for", xff.parse().unwrap());
    }
    send(app, req).await.status()
}

#[sqlx::test]
async fn one_ip_flooding_the_login_endpoint_is_rate_limited(pool: PgPool) {
    let app = test_app(pool);
    for i in 0..BURST {
        assert_ne!(
            cheap_request(&app, "198.51.100.7", None).await,
            StatusCode::TOO_MANY_REQUESTS,
            "request #{i}"
        );
    }
    let mut req = json_req("POST", "/api/auth/login", None, Some(json!({})));
    req.extensions_mut().insert(ConnectInfo(SocketAddr::new(
        "198.51.100.7".parse().unwrap(),
        40_000,
    )));
    let res = send(&app, req).await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    // 與其他限流相同的錯誤格式，前端不用改
    assert_eq!(json_body(res).await["error"]["code"], "too_many_attempts");

    // 別的 IP 不受影響
    assert_ne!(
        cheap_request(&app, "203.0.113.20", None).await,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[sqlx::test]
async fn the_rate_limit_is_per_forwarded_client_behind_a_trusted_proxy(pool: PgPool) {
    let app = test_app_with_config(pool, FakeAi::with(vec![]), config_trusting("127.0.0.0/8"));
    for _ in 0..BURST {
        cheap_request(&app, "127.0.0.1", Some("203.0.113.9")).await;
    }
    assert_eq!(
        cheap_request(&app, "127.0.0.1", Some("203.0.113.9")).await,
        StatusCode::TOO_MANY_REQUESTS
    );
    // 同一個代理後面的其他用戶端不是被一起擋
    assert_ne!(
        cheap_request(&app, "127.0.0.1", Some("203.0.113.10")).await,
        StatusCode::TOO_MANY_REQUESTS
    );
}
