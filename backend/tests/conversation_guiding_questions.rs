//! F-32.4 對話詳情附上該題目目前的引導問題（S-04.8），讓對話頁能顯示其中一題。

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test]
async fn conversation_detail_returns_topic_guiding_questions(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/topics",
            Some(&a.teacher),
            Some(json!({"title": "自由意志", "guiding_questions": ["問題一", "問題二"]})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let topic = json_body(res).await["id"].as_str().unwrap().to_owned();
    let activity = publish_activity(&app, &a, "活動", &topic).await;
    let conv = start_from_activity(&app, &a.student, &activity).await;

    let res = send(
        &app,
        get(&format!("/api/conversations/{conv}"), Some(&a.student)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        json_body(res).await["guiding_questions"],
        json!(["問題一", "問題二"])
    );

    // 教師之後修改引導問題，對話詳情跟著變（讀的是題目目前的值）
    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/topics/{topic}"),
            Some(&a.teacher),
            Some(json!({"guiding_questions": ["新問題"]})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let res = send(
        &app,
        get(&format!("/api/conversations/{conv}"), Some(&a.student)),
    )
    .await;
    assert_eq!(json_body(res).await["guiding_questions"], json!(["新問題"]));
}

#[sqlx::test]
async fn conversation_detail_guiding_questions_empty_without_any(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;

    let res = send(
        &app,
        get(&format!("/api/conversations/{conv}"), Some(&a.student)),
    )
    .await;
    assert_eq!(json_body(res).await["guiding_questions"], json!([]));
}
