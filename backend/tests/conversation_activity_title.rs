//! 對話 API 帶活動名稱（`activity_title`），讓「我的對話」來源欄能顯示活動名稱與題目。

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

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
    let topic = seed_topic("電車難題").await;
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
async fn activity_source_and_transcript_use_saved_titles_when_requested(pool: PgPool) {
    let app = test_app(pool.clone());
    let actors = seed_actors(&app).await;
    let topic = seed_topic("原題名").await;
    let activity = publish_activity(&app, &actors, "原活動", &topic).await;
    let conversation = start_from_activity(&app, &actors.student, &activity).await;
    sqlx::query(r#"UPDATE activities SET translations = '{"en":{"title":"Localized activity"}}' WHERE id = $1::uuid"#)
        .bind(&activity).execute(&pool).await.unwrap();
    sqlx::query(r#"UPDATE topics SET translations = '{"en":{"title":"Localized topic"}}' WHERE id = $1::uuid"#)
        .bind(&topic).execute(&pool).await.unwrap();

    let items = all_items(&app, "/api/conversations?language=en", &actors.student).await;
    assert_eq!(items[0]["activity_title"], "Localized activity");
    let items = all_items(&app, "/api/conversations?language=es", &actors.student).await;
    assert_eq!(items[0]["activity_title"], "原活動");

    sqlx::query("UPDATE conversations SET status = 'ended', ended_at = now() WHERE id = $1::uuid")
        .bind(&conversation)
        .execute(&pool)
        .await
        .unwrap();
    let path = format!("/api/summaries/{conversation}/messages?language=en");
    let response = send(&app, get(&path, Some(&actors.teacher))).await;
    assert_eq!(response.status(), StatusCode::OK);
    let transcript = json_body(response).await;
    assert_eq!(transcript["title"], "Localized topic");
    assert_eq!(transcript["language"], "zh-TW");
}

#[sqlx::test]
async fn personal_practice_has_no_activity_title(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let topic = seed_topic("自由意志").await;

    let created = start(&app, &a.student, json!({"topic_id": topic})).await;
    assert!(created["activity_title"].is_null());

    let items = all_items(&app, "/api/conversations", &a.student).await;
    assert!(items[0]["activity_title"].is_null());
}

#[sqlx::test]
async fn renaming_the_activity_updates_the_title_on_existing_conversations(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let topic = seed_topic("電車難題").await;
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
