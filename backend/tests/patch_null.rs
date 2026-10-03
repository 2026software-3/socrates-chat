//! PATCH 的三態語意：沒帶＝保留、`null`＝清除、有值＝設定（可為空的欄位；合成資料）。

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

async fn create(app: &axum::Router, cookie: &str, path: &str, body: Value) -> String {
    let res = send(app, json_req("POST", path, Some(cookie), Some(body))).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    json_body(res).await["id"].as_str().unwrap().to_string()
}

async fn patch(app: &axum::Router, cookie: &str, path: &str, body: Value) -> Value {
    let res = send(app, json_req("PATCH", path, Some(cookie), Some(body))).await;
    assert_eq!(res.status(), StatusCode::OK);
    json_body(res).await
}

#[sqlx::test]
async fn null_clears_topic_category_and_absent_keeps_it(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let id = create(
        &app,
        &a.admin,
        "/api/admin/topics",
        json!({"title": "T", "category": "倫理學"}),
    )
    .await;
    let path = format!("/api/admin/topics/{id}");

    let body = patch(&app, &a.admin, &path, json!({"title": "T2"})).await;
    assert_eq!(body["category"], "倫理學");

    let body = patch(&app, &a.admin, &path, json!({"category": null})).await;
    assert_eq!(body["category"], Value::Null);
    assert_eq!(body["title"], "T2");

    let body = patch(&app, &a.admin, &path, json!({"category": "知識論"})).await;
    assert_eq!(body["category"], "知識論");
}

#[sqlx::test]
async fn null_unlinks_activity_topic_and_absent_keeps_it(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let topic = create(&app, &a.admin, "/api/admin/topics", json!({"title": "T"})).await;
    let id = create(
        &app,
        &a.teacher,
        "/api/activities",
        json!({"title": "A", "topic_id": topic}),
    )
    .await;
    let path = format!("/api/activities/{id}");

    let body = patch(&app, &a.teacher, &path, json!({"title": "A2"})).await;
    assert_eq!(body["topic_id"], topic.as_str());

    let body = patch(&app, &a.teacher, &path, json!({"topic_id": null})).await;
    assert_eq!(body["topic_id"], Value::Null);
    assert_eq!(body["title"], "A2");
}
