//! `GET /api/admin/metrics` 的角色授權與維護模式整合測試（需 PostgreSQL）。

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use common::*;
use serde_json::json;
use socrates_chat_backend::{config::Config, metrics::PROMETHEUS_CONTENT_TYPE};
use sqlx::PgPool;

const METRICS: &str = "/api/admin/metrics";
const TOKEN: &str = "synthetic-db-metrics-token-012345";

#[sqlx::test]
async fn only_admins_or_valid_bearer_token_can_scrape_metrics(pool: PgPool) {
    let config = Config {
        metrics_token: Some(TOKEN.into()),
        ..test_config()
    };
    let app = test_app_with_config(pool, FakeAi::with(vec![]), config);
    let actors = seed_actors(&app).await;

    for (cookie, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(actors.teacher.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.student.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.outsider.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.admin.as_str()), StatusCode::OK),
    ] {
        let res = send(&app, get(METRICS, cookie)).await;
        assert_eq!(res.status(), expected);
        if expected == StatusCode::OK {
            assert_eq!(res.headers()[header::CONTENT_TYPE], PROMETHEUS_CONTENT_TYPE);
            let body = text_body(res).await;
            assert!(body.contains("socrates_http_requests_total"));
        }
    }

    // 即使反向代理附帶無關的 Authorization 標頭，持有有效管理者 session cookie 仍可存取
    let with_proxy_auth = send(
        &app,
        Request::get(METRICS)
            .header(header::AUTHORIZATION, "Basic dXNlcjpwYXNz")
            .header(header::COOKIE, &actors.admin)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(with_proxy_auth.status(), StatusCode::OK);

    // 維護模式開啟時，使用有效 METRICS_TOKEN 的監控抓取仍可正常取得指標
    let enable = send(
        &app,
        json_req(
            "PUT",
            "/api/admin/maintenance",
            Some(&actors.admin),
            Some(json!({"enabled": true})),
        ),
    )
    .await;
    assert_eq!(enable.status(), StatusCode::OK);

    let res = send(
        &app,
        Request::get(METRICS)
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = text_body(res).await;
    assert!(body.contains("socrates_maintenance_mode 1"));
}
