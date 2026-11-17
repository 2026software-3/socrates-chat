//! 送出訊息與保存 AI 回覆時的 claim（`generating_since`）、回合數與自動結束（假 AI、合成資料）。
//! 鎖住合併 SQL 前後必須相同的行為：並行時只有一則成功、到達回合上限時結束並建立總結、
//! 回覆保存後 claim 已釋放、失敗或回覆對不上時 claim 也會釋放。

mod common;

use axum::{Router, http::StatusCode};
use common::Script::*;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const GOOD_SUMMARY: &str = "{\"stance\":\"會拉桿\",\"reasons\":\"救五人優先\"}";

struct Row {
    status: String,
    stage: i16,
    turn_count: i32,
    generating: bool,
    ended: bool,
}

async fn row(pool: &PgPool, conv: &str) -> Row {
    let id: Uuid = conv.parse().unwrap();
    let (status, stage, turn_count, generating, ended): (String, i16, i32, bool, bool) =
        sqlx::query_as(
            "SELECT status, stage, turn_count, generating_since IS NOT NULL,
                    ended_at IS NOT NULL
             FROM conversations WHERE id = $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
    Row {
        status,
        stage,
        turn_count,
        generating,
        ended,
    }
}

async fn messages_of(pool: &PgPool, conv: &str, role: &str) -> i64 {
    let id: Uuid = conv.parse().unwrap();
    let (n,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM messages WHERE conversation_id = $1 AND role = $2")
            .bind(id)
            .bind(role)
            .fetch_one(pool)
            .await
            .unwrap();
    n
}

async fn summaries(pool: &PgPool, conv: &str) -> i64 {
    let id: Uuid = conv.parse().unwrap();
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM summaries WHERE conversation_id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
    n
}

async fn post(app: &Router, cookie: &str, conv: &str, text: &str) -> (StatusCode, Value) {
    let res = send(
        app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/messages"),
            Some(cookie),
            Some(json!({"content": text})),
        ),
    )
    .await;
    let status = res.status();
    if status == StatusCode::OK {
        // 讀完整條串流：第一個事件是保存好的學生訊息，最後一個是 done／error
        let events = parse_sse(&text_body(res).await);
        (status, json!(events))
    } else {
        (status, json_body(res).await)
    }
}

async fn wait_for_claim(pool: &PgPool, conv: &str) {
    for _ in 0..200 {
        if row(pool, conv).await.generating {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("the conversation was never claimed");
}

#[sqlx::test]
async fn two_simultaneous_messages_save_only_one(pool: PgPool) {
    // AI 不回應：第一則的 claim 會持續到逾時，兩個請求一定重疊
    let ai = FakeAi::with(vec![Hang, Hang]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;

    let (r1, r2) = tokio::join!(
        post(&app, &a.student, &conv, "甲"),
        post(&app, &a.student, &conv, "乙"),
    );
    let mut statuses = [r1.0, r2.0];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::CONFLICT]);
    let refused = if r1.0 == StatusCode::CONFLICT {
        &r1.1
    } else {
        &r2.1
    };
    assert_eq!(refused["error"]["code"], "reply_in_progress");

    // 只保存一則、只算一回合、只呼叫一輪 AI（含一次自動重試）
    assert_eq!(messages_of(&pool, &conv, "student").await, 1);
    let r = row(&pool, &conv).await;
    assert_eq!(r.turn_count, 1);
    assert_eq!(r.status, "active");
    // 失敗收尾後釋放
    assert!(!r.generating);
    assert_eq!(ai.request_count(), 2);
}

#[sqlx::test]
async fn a_saved_reply_releases_the_claim_and_records_the_stage(pool: PgPool) {
    let adv = || {
        Reply(vec![
            "為什麼？<<<META>>>{\"question_type\":\"reason\",\"advance\":true}",
        ])
    };
    let ai = FakeAi::with(vec![adv(), adv()]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;

    let (st, events) = post(&app, &a.student, &conv, "一").await;
    assert_eq!(st, StatusCode::OK);
    let last = events.as_array().unwrap().last().unwrap().clone();
    assert_eq!(last[0], "done");
    assert_eq!(last[1]["stage"], 2);

    // 收到 done 時 claim 已釋放，階段已寫入
    let r = row(&pool, &conv).await;
    assert!(!r.generating);
    assert_eq!(r.stage, 2);
    assert_eq!(r.turn_count, 1);
    assert_eq!(messages_of(&pool, &conv, "ai").await, 1);

    // 緊接著送下一則不會撞上 claim
    let (st, events) = post(&app, &a.student, &conv, "二").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(events.as_array().unwrap().last().unwrap()[0], "done");
    let r = row(&pool, &conv).await;
    assert!(!r.generating);
    assert_eq!(r.stage, 3);
    assert_eq!(r.turn_count, 2);
}

#[sqlx::test]
async fn the_last_turn_ends_the_conversation_and_creates_its_summary(pool: PgPool) {
    // 測試設定的 max_turns = 5；前 4 回合直接寫入
    let ai = FakeAi::with(vec![Reply(vec![GOOD_SUMMARY])]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "前面").await;
    sqlx::query("UPDATE conversations SET turn_count = 4 WHERE id = $1")
        .bind(conv.parse::<Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();

    let (st, events) = post(&app, &a.student, &conv, "最後").await;
    assert_eq!(st, StatusCode::OK);
    let events = events.as_array().unwrap().clone();
    // 只有保存好的學生訊息，不產生 AI 回覆
    assert_eq!(events.len(), 1);
    assert_eq!(events[0][0], "message");
    assert_eq!(events[0][1]["content"], "最後");
    assert_eq!(events[0][1]["auto_ended"], true);

    let r = row(&pool, &conv).await;
    assert_eq!(r.status, "ended");
    assert!(r.ended);
    assert!(!r.generating);
    assert_eq!(r.turn_count, 5);
    assert_eq!(messages_of(&pool, &conv, "student").await, 2);
    assert_eq!(messages_of(&pool, &conv, "ai").await, 0);
    assert_eq!(summaries(&pool, &conv).await, 1);
    assert_eq!(
        settled_summary(&app, &a.student, &conv).await["status"],
        "ready"
    );
    // 總結以外沒有呼叫 AI
    assert_eq!(ai.request_count(), 1);

    let (st, body) = post(&app, &a.student, &conv, "再一則").await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "conversation_ended");
}

#[sqlx::test]
async fn two_simultaneous_last_turn_messages_end_the_conversation_once(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD_SUMMARY])]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    sqlx::query("UPDATE conversations SET turn_count = 4 WHERE id = $1")
        .bind(conv.parse::<Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();

    let (r1, r2) = tokio::join!(
        post(&app, &a.student, &conv, "甲"),
        post(&app, &a.student, &conv, "乙"),
    );
    let mut statuses = [r1.0, r2.0];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::CONFLICT]);
    let (ok, refused) = if r1.0 == StatusCode::OK {
        (&r1.1, &r2.1)
    } else {
        (&r2.1, &r1.1)
    };
    assert_eq!(ok[0][1]["auto_ended"], true);
    // 等鎖後才發現已結束是 reply_in_progress；請求晚到、先讀到已結束是 conversation_ended
    let code = refused["error"]["code"].as_str().unwrap();
    assert!(
        code == "reply_in_progress" || code == "conversation_ended",
        "{code}"
    );

    let r = row(&pool, &conv).await;
    assert_eq!(r.status, "ended");
    assert_eq!(r.turn_count, 5);
    assert_eq!(messages_of(&pool, &conv, "student").await, 1);
    assert_eq!(summaries(&pool, &conv).await, 1);
}

#[sqlx::test]
async fn a_reply_overtaken_by_another_is_not_saved_and_releases_the_claim(pool: PgPool) {
    let pace = std::time::Duration::from_millis(150);
    let ai = FakeAi::with(vec![Paced(
        vec![
            "你說",
            "的是？<<<META>>>{\"question_type\":\"clarify\",\"advance\":true}",
        ],
        pace,
    )]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;

    let (app2, cookie, conv2) = (app.clone(), a.student.clone(), conv.clone());
    let pending = tokio::spawn(async move { post(&app2, &cookie, &conv2, "嗨").await });
    wait_for_claim(&pool, &conv).await;
    // 回覆產生期間，最後一則已不是學生訊息（例如別處已補上回覆）
    sqlx::query(
        "INSERT INTO messages (conversation_id, role, content) VALUES ($1, 'ai', '別處的回覆')",
    )
    .bind(conv.parse::<Uuid>().unwrap())
    .execute(&pool)
    .await
    .unwrap();

    let (st, events) = pending.await.unwrap();
    assert_eq!(st, StatusCode::OK);
    let last = events.as_array().unwrap().last().unwrap().clone();
    assert_eq!(last[0], "error");
    assert_eq!(last[1]["code"], "conflict");

    // 沒有寫入回覆、階段不變，claim 已釋放
    assert_eq!(messages_of(&pool, &conv, "ai").await, 1);
    let r = row(&pool, &conv).await;
    assert_eq!(r.stage, 1);
    assert!(!r.generating);
    assert_eq!(r.status, "active");
}

#[sqlx::test]
async fn a_failed_reply_releases_the_claim_and_keeps_the_message(pool: PgPool) {
    let ai = FakeAi::with(vec![Fail, Fail, Reply(vec!["然後呢？"])]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;

    let (st, events) = post(&app, &a.student, &conv, "嗨").await;
    assert_eq!(st, StatusCode::OK);
    let last = events.as_array().unwrap().last().unwrap().clone();
    assert_eq!(last[1]["code"], "ai_unavailable");

    let r = row(&pool, &conv).await;
    assert!(!r.generating);
    assert_eq!(r.turn_count, 1);
    assert_eq!(messages_of(&pool, &conv, "student").await, 1);
    assert_eq!(messages_of(&pool, &conv, "ai").await, 0);

    // 釋放後可以直接送下一則
    let (st, events) = post(&app, &a.student, &conv, "再試").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(events.as_array().unwrap().last().unwrap()[0], "done");
    assert!(!row(&pool, &conv).await.generating);
}
