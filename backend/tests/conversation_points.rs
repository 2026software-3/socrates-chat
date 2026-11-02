//! F-19.2 對話完成發點（S-07.2）：總結產生成功時，在同一個交易內入帳 10 點（假 AI、合成資料）。
//!
//! 刪除對話後重新完成是否再次發點尚待確認（S-04.7 待確認項目），這裡不測試、也不固定那種情況的行為。

mod common;

use std::time::Duration;

use axum::{Router, http::StatusCode};
use common::Script::*;
use common::*;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

const GOOD: &str = "{\"stance\":\"會拉桿\",\"reasons\":\"救五人優先\"}";

/// 輪詢到總結不再是 pending 為止（背景產生）。
async fn settled_summary(app: &Router, cookie: &str, conv: &str) -> Value {
    for _ in 0..100 {
        let res = send(
            app,
            get(&format!("/api/conversations/{conv}/summary"), Some(cookie)),
        )
        .await;
        let v = json_body(res).await;
        if v["status"] != "pending" {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("summary stayed pending");
}

async fn post(app: &Router, cookie: &str, path: &str) -> StatusCode {
    send(app, json_req("POST", path, Some(cookie), None))
        .await
        .status()
}

async fn balance(app: &Router, cookie: &str) -> i64 {
    let res = send(app, get("/api/points", Some(cookie))).await;
    assert_eq!(res.status(), StatusCode::OK);
    json_body(res).await["balance"].as_i64().unwrap()
}

/// 帳本中這位使用者的紀錄：(delta, reason, source_type, source_id)。
async fn ledger(pool: &PgPool, email: &str) -> Vec<(i32, String, String, String)> {
    sqlx::query_as(
        "SELECT l.delta, l.reason, l.source_type, l.source_id
         FROM point_ledger l JOIN users u ON u.id = l.user_id
         WHERE u.email = $1 ORDER BY l.created_at",
    )
    .bind(email)
    .fetch_all(pool)
    .await
    .unwrap()
}

/// 開始對話、說一句話並結束，等總結產生完成。
async fn finish_conversation(app: &Router, pool: &PgPool, a: &Actors) -> String {
    let conv = start_conversation(app, a).await;
    seed_student_message(pool, &conv, "我會拉桿，因為五個人比一個人多。").await;
    assert_eq!(
        post(app, &a.student, &format!("/api/conversations/{conv}/end")).await,
        StatusCode::ACCEPTED
    );
    conv
}

#[sqlx::test]
async fn completing_a_conversation_awards_ten_points_once(pool: PgPool) {
    let app = test_app_with_ai(pool.clone(), FakeAi::with(vec![Reply(vec![GOOD])]));
    let a = seed_actors(&app).await;
    assert_eq!(balance(&app, &a.student).await, 0);

    let conv = finish_conversation(&app, &pool, &a).await;
    assert_eq!(
        settled_summary(&app, &a.student, &conv).await["status"],
        "ready"
    );

    assert_eq!(balance(&app, &a.student).await, 10);
    // 帳本只存 ID 與數值，不含對話內容（S-07.2 2.1、S-02.2）
    assert_eq!(
        ledger(&pool, "student@example.com").await,
        vec![(
            10,
            "conversation_completed".to_string(),
            "conversation".to_string(),
            conv.clone()
        )]
    );
}

#[sqlx::test]
async fn a_failed_summary_awards_nothing_until_a_retry_succeeds(pool: PgPool) {
    // 第一輪（含自動重試）兩次都失敗，之後手動重試成功
    let app = test_app_with_ai(
        pool.clone(),
        FakeAi::with(vec![Fail, Fail, Reply(vec![GOOD])]),
    );
    let a = seed_actors(&app).await;
    let conv = finish_conversation(&app, &pool, &a).await;
    assert_eq!(
        settled_summary(&app, &a.student, &conv).await["status"],
        "failed"
    );
    // 未產生總結的對話不發點（S-07.2 2.2）
    assert_eq!(balance(&app, &a.student).await, 0);

    let retry = format!("/api/conversations/{conv}/summary/retry");
    assert_eq!(post(&app, &a.student, &retry).await, StatusCode::ACCEPTED);
    assert_eq!(
        settled_summary(&app, &a.student, &conv).await["status"],
        "ready"
    );
    assert_eq!(balance(&app, &a.student).await, 10);
}

#[sqlx::test]
async fn regenerating_the_same_conversation_does_not_award_again(pool: PgPool) {
    let app = test_app_with_ai(
        pool.clone(),
        FakeAi::with(vec![Reply(vec![GOOD]), Reply(vec![GOOD])]),
    );
    let a = seed_actors(&app).await;
    let conv = finish_conversation(&app, &pool, &a).await;
    settled_summary(&app, &a.student, &conv).await;
    assert_eq!(balance(&app, &a.student).await, 10);

    // 同一場對話的總結再次成功（例如被標成失敗後重試）：防重複，只入帳一次
    sqlx::query("UPDATE summaries SET status = 'failed' WHERE conversation_id = $1")
        .bind(Uuid::parse_str(&conv).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let retry = format!("/api/conversations/{conv}/summary/retry");
    assert_eq!(post(&app, &a.student, &retry).await, StatusCode::ACCEPTED);
    assert_eq!(
        settled_summary(&app, &a.student, &conv).await["status"],
        "ready"
    );
    assert_eq!(balance(&app, &a.student).await, 10);
    assert_eq!(ledger(&pool, "student@example.com").await.len(), 1);
}

#[sqlx::test]
async fn an_unfinished_conversation_awards_nothing(pool: PgPool) {
    let app = test_app_with_ai(pool.clone(), FakeAi::with(vec![]));
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "還在想").await;
    assert_eq!(balance(&app, &a.student).await, 0);
    assert!(ledger(&pool, "student@example.com").await.is_empty());
}

#[sqlx::test]
async fn points_go_only_to_the_conversation_owner(pool: PgPool) {
    let app = test_app_with_ai(pool.clone(), FakeAi::with(vec![Reply(vec![GOOD])]));
    let a = seed_actors(&app).await;
    let other = add_student(&app, &a, "sub-other", "other@example.com").await;
    let conv = finish_conversation(&app, &pool, &a).await;
    settled_summary(&app, &a.student, &conv).await;

    assert_eq!(balance(&app, &a.student).await, 10);
    assert_eq!(balance(&app, &other).await, 0);
    // 教師與管理者不會因學生完成對話而入帳
    for email in ["teacher@example.com", "admin@example.com"] {
        assert!(ledger(&pool, email).await.is_empty());
    }
}
