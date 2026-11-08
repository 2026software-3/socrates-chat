//! F-19.2 對話完成發點（S-07.2）：總結產生成功時，在同一個交易內入帳 10 點（假 AI、合成資料）。
//!
//! 刪除對話後重新完成是否再次發點尚待確認（S-04.7 待確認項目），這裡不測試、也不固定那種情況的行為。

mod common;

use std::time::Duration;

use axum::{Router, http::StatusCode};
use common::Script::*;
use common::*;
use socrates_chat_backend::config::Config;
use sqlx::PgPool;

const GOOD: &str = "{\"stance\":\"會拉桿\",\"reasons\":\"救五人優先\"}";

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
    let end = json_req(
        "POST",
        &format!("/api/conversations/{conv}/end"),
        Some(&a.student),
        None,
    );
    assert_eq!(send(app, end).await.status(), StatusCode::ACCEPTED);
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

    assert_eq!(balance(&app, &a.student).await, 20);
    // 帳本只存 ID 與數值，不含對話內容（S-07.2 2.1、S-02.2）
    assert_eq!(
        ledger(&pool, "student@example.com")
            .await
            .into_iter()
            .filter(|entry| entry.2 == "conversation")
            .collect::<Vec<_>>(),
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

    assert_eq!(
        retry_summary(&app, &a.student, &conv).await,
        StatusCode::ACCEPTED
    );
    assert_eq!(
        settled_summary(&app, &a.student, &conv).await["status"],
        "ready"
    );
    assert_eq!(balance(&app, &a.student).await, 20);
}

#[sqlx::test]
async fn a_superseded_generation_awards_nothing(pool: PgPool) {
    // 第一代很慢才回出好結果；在它完成前手動重試開出第二代，而第二代卡住。
    // 第一代完成時代數已不符，不能把總結改成 ready，也不能發點。
    let config = Config {
        ai_total_timeout: Duration::from_secs(3),
        ..test_config()
    };
    let ai = FakeAi::with(vec![Paced(vec![GOOD], Duration::from_millis(500)), Hang]);
    let app = test_app_with_config(pool.clone(), ai.clone(), config);
    let a = seed_actors(&app).await;
    let conv = finish_conversation(&app, &pool, &a).await;
    wait_until_ai_called(&ai, 1).await;
    sqlx::query("UPDATE summaries SET started_at = now() - interval '1 hour'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        retry_summary(&app, &a.student, &conv).await,
        StatusCode::ACCEPTED
    );

    // 等第一代的結果回來並被丟棄
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let res = send(
        &app,
        get(
            &format!("/api/conversations/{conv}/summary"),
            Some(&a.student),
        ),
    )
    .await;
    assert_eq!(json_body(res).await["status"], "pending");
    assert_eq!(balance(&app, &a.student).await, 0);
    assert!(ledger(&pool, "student@example.com").await.is_empty());
}

#[sqlx::test]
async fn a_failed_award_rolls_back_the_summary_and_a_retry_awards_once(pool: PgPool) {
    // 入帳失敗時，總結不能已經變成 ready（同一個交易，S-07.2 2.2）
    sqlx::query(
        "CREATE FUNCTION reject_ledger_insert() RETURNS trigger AS $$
         BEGIN RAISE EXCEPTION 'ledger unavailable'; END $$ LANGUAGE plpgsql",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER reject_ledger_insert BEFORE INSERT ON point_ledger
         FOR EACH ROW EXECUTE FUNCTION reject_ledger_insert()",
    )
    .execute(&pool)
    .await
    .unwrap();

    let app = test_app_with_ai(
        pool.clone(),
        FakeAi::with(vec![Reply(vec![GOOD]), Reply(vec![GOOD])]),
    );
    let a = seed_actors(&app).await;
    let conv = finish_conversation(&app, &pool, &a).await;
    assert_eq!(
        settled_summary(&app, &a.student, &conv).await["status"],
        "failed"
    );
    assert_eq!(balance(&app, &a.student).await, 0);

    sqlx::query("DROP TRIGGER reject_ledger_insert ON point_ledger")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        retry_summary(&app, &a.student, &conv).await,
        StatusCode::ACCEPTED
    );
    assert_eq!(
        settled_summary(&app, &a.student, &conv).await["status"],
        "ready"
    );
    assert_eq!(balance(&app, &a.student).await, 20);
    assert_eq!(ledger(&pool, "student@example.com").await.len(), 2);
}

#[sqlx::test]
async fn an_active_conversation_awards_nothing(pool: PgPool) {
    // 經由真正的送出訊息流程：AI 回覆完成但對話還沒結束
    let app = test_app_with_ai(pool.clone(), FakeAi::with(vec![Reply(vec!["為什麼？"])]));
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    let (status, _) = student_says(&app, &a, &conv, "我會拉桿").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(balance(&app, &a.student).await, 0);
    assert!(ledger(&pool, "student@example.com").await.is_empty());
}

#[sqlx::test]
async fn a_pending_summary_awards_nothing_yet(pool: PgPool) {
    // 對話已結束但總結還在產生：總結 ready 前不發點
    let ai = FakeAi::with(vec![Hang]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let a = seed_actors(&app).await;
    finish_conversation(&app, &pool, &a).await;
    wait_until_ai_called(&ai, 1).await;
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

    assert_eq!(balance(&app, &a.student).await, 20);
    assert_eq!(balance(&app, &other).await, 0);
    // 教師與管理者不會因學生完成對話而入帳
    for email in ["teacher@example.com", "admin@example.com"] {
        assert!(ledger(&pool, email).await.is_empty());
    }
}
