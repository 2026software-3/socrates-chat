mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;
#[sqlx::test]
async fn removed_admin_topic_operation_returns_404(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/topics",
            Some(&a.admin),
            Some(json!({"title":"T"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let doc = json_body(send(&app, get("/api/openapi.json", None)).await).await;
    assert!(doc["paths"]["/api/admin/topics"]["post"].is_null());
}
