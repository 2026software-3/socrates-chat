//! 暫時維運日誌頁：只讓管理者查看目前程序記憶體中的安全 request log。

mod common;

use axum::http::{StatusCode, header};
use common::*;
use sqlx::PgPool;

#[sqlx::test]
async fn admin_sees_recent_sanitized_request_logs_newest_first(pool: PgPool) {
    let app = test_app(pool);
    let actors = seed_actors(&app).await;

    let health = send(&app, get("/health", None)).await;
    let health_id = health.headers()[header::HeaderName::from_static("x-request-id")]
        .to_str()
        .unwrap()
        .to_string();
    let missing = send(&app, get("/private/student@example.com", None)).await;
    let missing_id = missing.headers()[header::HeaderName::from_static("x-request-id")]
        .to_str()
        .unwrap()
        .to_string();

    let response = send(&app, get("/api/admin/logs", Some(&actors.admin))).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["capacity"], 500);

    let items = body["items"].as_array().unwrap();
    assert_eq!(items[0]["request_id"], missing_id);
    assert_eq!(items[0]["method"], "GET");
    assert_eq!(items[0]["route"], "<fallback>");
    assert_eq!(items[0]["status"], 404);
    assert_eq!(items[0]["error_code"], "not_found");
    assert!(items[0]["duration_ms"].is_u64());
    assert!(items[0]["created_at"].is_string());

    assert_eq!(items[1]["request_id"], health_id);
    assert_eq!(items[1]["route"], "/health");
    assert_eq!(items[1]["status"], 200);
    assert!(items[1]["error_code"].is_null());
    assert!(!body.to_string().contains("student@example.com"));
}

#[sqlx::test]
async fn request_logs_are_admin_only(pool: PgPool) {
    let app = test_app(pool);
    let actors = seed_actors(&app).await;

    for (cookie, expected) in [
        (Some(actors.teacher.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.student.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.outsider.as_str()), StatusCode::FORBIDDEN),
        (None, StatusCode::UNAUTHORIZED),
    ] {
        let response = send(&app, get("/api/admin/logs", cookie)).await;
        assert_eq!(response.status(), expected);
    }

    let body = json_body(send(&app, get("/api/admin/logs", Some(&actors.admin))).await).await;
    let denied: Vec<_> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["route"] == "/api/admin/logs")
        .collect();
    assert_eq!(denied.len(), 4);
    assert_eq!(denied[0]["status"], 401);
    assert_eq!(denied[0]["error_code"], "unauthorized");
    assert!(
        denied[1..]
            .iter()
            .all(|entry| { entry["status"] == 403 && entry["error_code"] == "forbidden" })
    );
}

#[sqlx::test]
async fn request_logs_keep_only_the_latest_five_hundred_entries(pool: PgPool) {
    let app = test_app(pool);
    let actors = seed_actors(&app).await;

    for _ in 0..501 {
        assert_eq!(
            send(&app, get("/health", None)).await.status(),
            StatusCode::OK
        );
    }

    let body = json_body(send(&app, get("/api/admin/logs", Some(&actors.admin))).await).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 500);
    assert!(items.iter().all(|entry| entry["route"] == "/health"));
}
