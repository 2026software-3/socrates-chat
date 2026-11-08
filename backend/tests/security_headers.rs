//! 安全回應標頭：所有回應（含錯誤、API 文件）都要帶；HSTS 只在 HTTPS 部署送出。

mod common;

use axum::{
    Router,
    body::Body,
    http::{Request, Response},
};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

fn app_with_base_url(base_url: &str) -> Router {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://unused@127.0.0.1:1/unused")
        .unwrap();
    let mut config = common::test_config();
    config.app_base_url = base_url.to_string();
    common::test_app_with_config(pool, common::FakeAi::with(vec![]), config)
}

async fn get(app: Router, path: &str) -> Response<Body> {
    app.oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

fn header_str<'a>(res: &'a Response<Body>, name: &str) -> &'a str {
    res.headers()
        .get(name)
        .unwrap_or_else(|| panic!("缺少標頭 {name}"))
        .to_str()
        .unwrap()
}

#[tokio::test]
async fn ordinary_responses_carry_strict_security_headers() {
    let res = get(app_with_base_url("https://app.test"), "/health").await;

    assert_eq!(header_str(&res, "x-content-type-options"), "nosniff");
    assert_eq!(header_str(&res, "x-frame-options"), "DENY");
    assert_eq!(header_str(&res, "referrer-policy"), "no-referrer");
    let csp = header_str(&res, "content-security-policy");
    assert!(csp.contains("default-src 'self'"), "{csp}");
    assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
    assert!(csp.contains("object-src 'none'"), "{csp}");
    assert!(csp.contains("base-uri 'self'"), "{csp}");
    assert!(
        !csp.contains("script-src 'self' 'unsafe-inline'"),
        "一般頁面不可放行行內 script：{csp}"
    );
}

#[tokio::test]
async fn error_responses_also_carry_security_headers() {
    let res = get(app_with_base_url("https://app.test"), "/api/does-not-exist").await;

    assert_eq!(res.status(), 404);
    assert_eq!(header_str(&res, "x-content-type-options"), "nosniff");
    assert_eq!(header_str(&res, "x-frame-options"), "DENY");
    assert!(res.headers().contains_key("content-security-policy"));
}

#[tokio::test]
async fn hsts_is_sent_only_when_served_over_https() {
    let https = get(app_with_base_url("https://app.test"), "/health").await;
    assert_eq!(
        header_str(&https, "strict-transport-security"),
        "max-age=31536000"
    );

    let http = get(app_with_base_url("http://localhost:3000"), "/health").await;
    assert!(
        !http.headers().contains_key("strict-transport-security"),
        "http 部署不可送 HSTS"
    );
}

#[tokio::test]
async fn api_docs_page_gets_relaxed_script_policy_for_redoc() {
    let res = get(app_with_base_url("https://app.test"), "/api/docs").await;

    let csp = header_str(&res, "content-security-policy");
    assert!(csp.contains("script-src 'self' 'unsafe-inline'"), "{csp}");
    assert!(csp.contains("worker-src blob:"), "{csp}");
    assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
    assert_eq!(header_str(&res, "x-frame-options"), "DENY");
}

#[tokio::test]
async fn other_api_paths_keep_the_strict_script_policy() {
    let res = get(app_with_base_url("https://app.test"), "/api/openapi.json").await;

    let csp = header_str(&res, "content-security-policy");
    assert!(
        !csp.contains("script-src 'self' 'unsafe-inline'"),
        "只有 /api/docs 可放寬：{csp}"
    );
}
