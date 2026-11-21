//! F-29.6：`GET /api/catalog` 的活動帶每題標題與該學生的對話狀態、整體完成度（Issue #391、S-04.7）。

mod common;

use axum::http::StatusCode;
use common::Script::*;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

const GOOD: &str = r#"{"stance":"主張","reasons":"核心理由"}"#;

async fn catalog_activity(app: &axum::Router, cookie: &str, activity: &str) -> Value {
    let res = send(app, get("/api/catalog", Some(cookie))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    body["activities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == activity)
        .unwrap_or_else(|| panic!("activity {activity} missing from catalog"))
        .clone()
}

async fn start(app: &axum::Router, cookie: &str, activity: &str, topic: &str) -> String {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/conversations",
            Some(cookie),
            Some(json!({"activity_id": activity, "topic_id": topic})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    json_body(res).await["id"].as_str().unwrap().to_owned()
}

async fn finish(app: &axum::Router, pool: &PgPool, cookie: &str, conv: &str) {
    seed_student_message(pool, conv, "這是學生的哲學論點與回答").await;
    let res = send(
        app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/end"),
            Some(cookie),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    assert_eq!(settled_summary(app, cookie, conv).await["status"], "ready");
}

#[sqlx::test]
async fn catalog_shows_per_topic_progress_and_completion(pool: PgPool) {
    let ai = FakeAi::with(vec![
        Reply(vec![GOOD]),
        Reply(vec![GOOD]),
        Reply(vec![GOOD]),
    ]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let t1 = seed_topic("第一題").await;
    let t2 = seed_topic("第二題").await;
    let t3 = seed_topic("第三題").await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({
                "title": "三題活動",
                "classroom_id": a.classroom,
                "topic_ids": [&t1, &t2, &t3]
            })),
        ),
    )
    .await;
    let activity = json_body(res).await["id"].as_str().unwrap().to_owned();
    send(
        &app,
        json_req(
            "POST",
            &format!("/api/activities/{activity}/publish"),
            Some(&a.teacher),
            None,
        ),
    )
    .await;

    // 還沒開始：依教師排定順序列出三題，沒有對話，未完成
    let card = catalog_activity(&app, &a.student, &activity).await;
    let topics = card["topics"].as_array().unwrap();
    assert_eq!(topics.len(), 3);
    assert_eq!(
        topics
            .iter()
            .map(|t| t["title"].clone())
            .collect::<Vec<_>>(),
        vec![json!("第一題"), json!("第二題"), json!("第三題")]
    );
    assert_eq!(topics[0]["topic_id"], t1);
    assert!(topics.iter().all(|t| t["conversation_id"].is_null()));
    assert!(topics.iter().all(|t| t["summary_ready"] == false));
    assert_eq!(card["completed"], false);

    // 第一題進行中：帶對話 id 與狀態 active，尚無總結
    let c1 = start(&app, &a.student, &activity, &t1).await;
    let card = catalog_activity(&app, &a.student, &activity).await;
    let first = &card["topics"][0];
    assert_eq!(first["conversation_id"], c1);
    assert_eq!(first["conversation_status"], "active");
    assert_eq!(first["summary_ready"], false);
    assert!(card["topics"][1]["conversation_id"].is_null());

    // 完成 2 題：第 3 題仍可開始，活動尚未完成
    finish(&app, &pool, &a.student, &c1).await;
    let c2 = start(&app, &a.student, &activity, &t2).await;
    finish(&app, &pool, &a.student, &c2).await;
    let card = catalog_activity(&app, &a.student, &activity).await;
    assert_eq!(card["topics"][0]["conversation_status"], "ended");
    assert_eq!(card["topics"][0]["summary_ready"], true);
    assert_eq!(card["topics"][1]["summary_ready"], true);
    assert!(card["topics"][2]["conversation_id"].is_null());
    assert_eq!(card["completed"], false);

    // 完成第 3 題：已完成
    let c3 = start(&app, &a.student, &activity, &t3).await;
    finish(&app, &pool, &a.student, &c3).await;
    let card = catalog_activity(&app, &a.student, &activity).await;
    assert_eq!(card["completed"], true);
}

#[sqlx::test]
async fn progress_is_per_student(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let t1 = seed_topic("第一題").await;
    let activity = publish_activity(&app, &a, "單題活動", &t1).await;
    let other = add_student(&app, &a, "sub-other", "other@example.com").await;

    start(&app, &a.student, &activity, &t1).await;

    let mine = catalog_activity(&app, &a.student, &activity).await;
    assert!(!mine["topics"][0]["conversation_id"].is_null());
    let theirs = catalog_activity(&app, &other, &activity).await;
    assert!(theirs["topics"][0]["conversation_id"].is_null());
}
