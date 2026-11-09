//! 對話 API 帶活動名稱（`activity_title`），讓「我的對話」來源欄能顯示活動名稱與題目。

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

/// deploy 分支沒有 main 的分頁 helper（#418）；這裡每個測試只有一筆對話，讀第一頁即可。
async fn all_items(app: &axum::Router, base: &str, cookie: &str) -> Value {
    let res = send(app, get(base, Some(cookie))).await;
    assert_eq!(res.status(), StatusCode::OK);
    json_body(res).await["items"].clone()
}

async fn start(app: &axum::Router, cookie: &str, body: Value) -> Value {
    let res = send(
        app,
        json_req("POST", "/api/conversations", Some(cookie), Some(body)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    json_body(res).await
}

#[sqlx::test]
async fn activity_conversation_carries_the_activity_title_next_to_the_topic_title(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let topic = create_topic(&app, &a, "電車難題").await;
    let activity = publish_activity(&app, &a, "倫理學第一週", &topic).await;

    // 建立回應
    let created = start(
        &app,
        &a.student,
        json!({"activity_id": activity, "topic_id": topic}),
    )
    .await;
    assert_eq!(created["activity_title"], "倫理學第一週");
    assert_eq!(created["title"], "電車難題");
    let id = created["id"].as_str().unwrap();

    // 單場讀取
    let res = send(
        &app,
        get(&format!("/api/conversations/{id}"), Some(&a.student)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_body(res).await["activity_title"], "倫理學第一週");

    // 列表
    let items = all_items(&app, "/api/conversations", &a.student).await;
    assert_eq!(items[0]["activity_title"], "倫理學第一週");
}

#[sqlx::test]
async fn personal_practice_has_no_activity_title(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let topic = create_topic(&app, &a, "自由意志").await;

    let created = start(&app, &a.student, json!({"topic_id": topic})).await;
    assert!(created["activity_title"].is_null());

    let items = all_items(&app, "/api/conversations", &a.student).await;
    assert!(items[0]["activity_title"].is_null());
}

#[sqlx::test]
async fn renaming_the_activity_updates_the_title_on_existing_conversations(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let topic = create_topic(&app, &a, "電車難題").await;
    let activity = publish_activity(&app, &a, "舊名稱", &topic).await;
    let created = start(
        &app,
        &a.student,
        json!({"activity_id": activity, "topic_id": topic}),
    )
    .await;
    let id = created["id"].as_str().unwrap();

    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/activities/{activity}"),
            Some(&a.teacher),
            Some(json!({"title": "新名稱"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    let res = send(
        &app,
        get(&format!("/api/conversations/{id}"), Some(&a.student)),
    )
    .await;
    assert_eq!(json_body(res).await["activity_title"], "新名稱");
}
