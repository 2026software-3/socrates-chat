//! F-08 結束與總結、F-26 教師檢視：行為、失敗處理與授權（假 AI、合成資料）。

mod common;

use std::time::Duration;

use axum::{Router, http::StatusCode};
use common::Script::*;
use common::*;
use serde_json::{Value, json};
use socrates_chat_backend::config::Config;
use sqlx::PgPool;

const GOOD: &str = "{\"stance\":\"會拉桿\",\"reasons\":\"救五人優先\"}";
const INCOMPLETE: &str = "{\"stance\":\"會拉桿\"}";

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

async fn end(app: &Router, cookie: &str, conv: &str) -> (StatusCode, Value) {
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
    let s = res.status();
    (s, json_body(res).await)
}

#[sqlx::test]
async fn ending_generates_a_saved_summary(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool, ai.clone());
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    student_says(&app, &a, &conv, "我會拉桿，因為五個人比一個人多。").await;

    let (status, body) = end(&app, &a.student, &conv).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body["status"], "pending");

    let s = settled_summary(&app, &a.student, &conv).await;
    assert_eq!(s["status"], "ready");
    assert_eq!(s["stance"], "會拉桿");
    assert_eq!(s["reasons"], "救五人優先");

    // 對話已結束，不能再送訊息；AI 收到的是完整逐字稿
    let (st, _) = student_says(&app, &a, &conv, "還想補充").await;
    assert_eq!(st, StatusCode::CONFLICT);
    let reqs = ai.requests.lock().unwrap();
    assert!(reqs[0].messages[0].content.contains("學生：我會拉桿"));
    assert!(reqs[0].system.contains("電車難題"));
}

#[sqlx::test]
async fn cannot_end_an_empty_conversation_and_ending_twice_is_idempotent(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD]), Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool, ai.clone());
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    let (st, body) = end(&app, &a.student, &conv).await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "empty_conversation");

    student_says(&app, &a, &conv, "想法").await;
    assert_eq!(end(&app, &a.student, &conv).await.0, StatusCode::ACCEPTED);
    settled_summary(&app, &a.student, &conv).await;
    assert_eq!(end(&app, &a.student, &conv).await.0, StatusCode::OK);
    assert_eq!(ai.request_count(), 1, "不會重複產生總結");
}

#[sqlx::test]
async fn failed_summary_is_marked_and_can_be_retried(pool: PgPool) {
    // 第一輪（含自動重試）兩次都失敗，之後手動重試成功
    let ai = FakeAi::with(vec![Fail, Fail, Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool, ai.clone());
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    student_says(&app, &a, &conv, "想法").await;
    end(&app, &a.student, &conv).await;

    let s = settled_summary(&app, &a.student, &conv).await;
    assert_eq!(s["status"], "failed");
    assert!(s["stance"].is_null());
    assert_eq!(ai.request_count(), 2);

    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/summary/retry"),
            Some(&a.student),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    assert_eq!(
        settled_summary(&app, &a.student, &conv).await["status"],
        "ready"
    );

    // 完成後不能再重試
    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/summary/retry"),
            Some(&a.student),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(res).await["error"]["code"],
        "summary_not_retryable"
    );
}

#[sqlx::test]
async fn incomplete_ai_result_counts_as_failure(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![INCOMPLETE]), Reply(vec!["不是 JSON"])]);
    let app = test_app_with_ai(pool, ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    student_says(&app, &a, &conv, "想法").await;
    end(&app, &a.student, &conv).await;
    let s = settled_summary(&app, &a.student, &conv).await;
    assert_eq!(s["status"], "failed");
    assert!(s["stance"].is_null() && s["reasons"].is_null());
}

#[sqlx::test]
async fn first_attempt_incomplete_then_complete_is_ready(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![INCOMPLETE]), Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool, ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    student_says(&app, &a, &conv, "想法").await;
    end(&app, &a.student, &conv).await;
    assert_eq!(
        settled_summary(&app, &a.student, &conv).await["status"],
        "ready"
    );
}

#[sqlx::test]
async fn stale_pending_summary_can_be_retried_but_fresh_one_cannot(pool: PgPool) {
    let ai = FakeAi::with(vec![Hang, Hang, Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    student_says(&app, &a, &conv, "想法").await;
    end(&app, &a.student, &conv).await;
    sqlx::query("UPDATE summaries SET status = 'pending', started_at = now()")
        .execute(&pool)
        .await
        .unwrap();
    let retry = || {
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/summary/retry"),
            Some(&a.student),
            None,
        )
    };
    assert_eq!(send(&app, retry()).await.status(), StatusCode::CONFLICT);
    sqlx::query("UPDATE summaries SET started_at = now() - interval '1 hour'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(send(&app, retry()).await.status(), StatusCode::ACCEPTED);
}

#[sqlx::test]
async fn reaching_turn_limit_ends_conversation_and_summarises(pool: PgPool) {
    // 測試設定的 max_turns = 5
    let ai = FakeAi::with(vec![Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool, ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    for i in 1..5 {
        let (st, body) = student_says(&app, &a, &conv, &format!("第{i}則")).await;
        assert_eq!(st, StatusCode::CREATED);
        assert_eq!(body["auto_ended"], false);
    }
    let (st, body) = student_says(&app, &a, &conv, "第5則").await;
    assert_eq!(st, StatusCode::CREATED);
    assert_eq!(body["auto_ended"], true);
    assert_eq!(body["content"], "第5則");

    assert_eq!(
        settled_summary(&app, &a.student, &conv).await["status"],
        "ready"
    );
    let d = json_body(
        send(
            &app,
            get(&format!("/api/conversations/{conv}"), Some(&a.student)),
        )
        .await,
    )
    .await;
    assert_eq!(d["status"], "ended");
    assert_eq!(
        student_says(&app, &a, &conv, "第6則").await.0,
        StatusCode::CONFLICT
    );
}

#[sqlx::test]
async fn summary_endpoints_are_owner_only_and_read_only(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool, ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    student_says(&app, &a, &conv, "想法").await;
    end(&app, &a.student, &conv).await;
    settled_summary(&app, &a.student, &conv).await;

    for cookie in [&a.teacher, &a.admin] {
        let res = send(
            &app,
            get(&format!("/api/conversations/{conv}/summary"), Some(cookie)),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(end(&app, cookie, &conv).await.0, StatusCode::FORBIDDEN);
        let res = send(
            &app,
            json_req(
                "POST",
                &format!("/api/conversations/{conv}/summary/retry"),
                Some(cookie),
                None,
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }
    let res = send(
        &app,
        get(
            &format!("/api/conversations/{conv}/summary"),
            Some(&a.outsider),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let res = send(
        &app,
        get(&format!("/api/conversations/{conv}/summary"), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // 學生不能修改總結：沒有任何寫入端點
    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/conversations/{conv}/summary"),
            Some(&a.student),
            Some(json!({"stance": "改掉"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    let s = settled_summary(&app, &a.student, &conv).await;
    assert_eq!(s["stance"], "會拉桿");
}

#[sqlx::test]
async fn teacher_sees_identified_summaries_but_never_message_text(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool, ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    student_says(&app, &a, &conv, "這句是私密的對話原文").await;
    // 另一場尚未結束、以及一場失敗的，都不該出現
    let _active = start_conversation(&app, &a).await;
    end(&app, &a.student, &conv).await;
    settled_summary(&app, &a.student, &conv).await;

    let res = send(&app, get("/api/teacher/summaries", Some(&a.teacher))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let list = json_body(res).await;
    let rows = list.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["conversation_id"], conv);
    assert_eq!(rows[0]["student_email"], "student@example.com");
    assert_eq!(rows[0]["student_name"], "User sub-student");
    assert_eq!(rows[0]["stance"], "會拉桿");
    assert!(!list.to_string().contains("私密的對話原文"));

    // 學生被移出名單後，教師仍看得到過去的總結（S-01.4）
    send(
        &app,
        json_req(
            "DELETE",
            "/api/roster/student@example.com",
            Some(&a.admin),
            None,
        ),
    )
    .await;
    let list = json_body(send(&app, get("/api/teacher/summaries", Some(&a.teacher))).await).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
}

#[sqlx::test]
async fn admin_view_of_summaries_is_masked(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool, ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    student_says(&app, &a, &conv, "想法").await;
    end(&app, &a.student, &conv).await;
    settled_summary(&app, &a.student, &conv).await;

    let list = json_body(send(&app, get("/api/teacher/summaries", Some(&a.admin))).await).await;
    let row = &list[0];
    for f in ["stance", "reasons"] {
        assert_eq!(row[f], "message");
    }
    assert!(!list.to_string().contains("會拉桿"));
}

#[sqlx::test]
async fn teacher_summary_list_denies_everyone_else(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    for (cookie, expected) in [
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        (None, StatusCode::UNAUTHORIZED),
    ] {
        assert_eq!(
            send(&app, get("/api/teacher/summaries", cookie))
                .await
                .status(),
            expected
        );
    }
}

#[sqlx::test]
async fn deleting_conversation_removes_summary_from_teacher_view(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool, ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    student_says(&app, &a, &conv, "想法").await;
    end(&app, &a.student, &conv).await;
    settled_summary(&app, &a.student, &conv).await;
    let res = send(
        &app,
        json_req(
            "DELETE",
            &format!("/api/conversations/{conv}"),
            Some(&a.student),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let list = json_body(send(&app, get("/api/teacher/summaries", Some(&a.teacher))).await).await;
    assert!(list.as_array().unwrap().is_empty());
}

#[sqlx::test]
async fn admin_who_is_also_teacher_still_sees_masked_summaries(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool, ai);
    let a = seed_actors(&app).await;
    // 管理者同時被加為教師
    send(
        &app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(&a.admin),
            Some(json!({"email": "admin@example.com"})),
        ),
    )
    .await;
    let conv = start_conversation(&app, &a).await;
    student_says(&app, &a, &conv, "想法").await;
    end(&app, &a.student, &conv).await;
    settled_summary(&app, &a.student, &conv).await;

    let list = json_body(send(&app, get("/api/teacher/summaries", Some(&a.admin))).await).await;
    assert_eq!(list[0]["stance"], "message");
    assert!(!list.to_string().contains("會拉桿"));
}

#[sqlx::test]
async fn teacher_list_only_contains_student_conversations(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool, ai);
    let a = seed_actors(&app).await;
    // 教師不參與討論：不能開對話
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({"title": "試用"})),
        ),
    )
    .await;
    let act = json_body(res).await["id"].as_str().unwrap().to_string();
    send(
        &app,
        json_req(
            "POST",
            &format!("/api/activities/{act}/publish"),
            Some(&a.teacher),
            None,
        ),
    )
    .await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/conversations",
            Some(&a.teacher),
            Some(json!({"activity_id": act})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 學生的對話才會出現
    let conv = start_conversation(&app, &a).await;
    student_says(&app, &a, &conv, "想法").await;
    end(&app, &a.student, &conv).await;
    settled_summary(&app, &a.student, &conv).await;

    let list = json_body(send(&app, get("/api/teacher/summaries", Some(&a.teacher))).await).await;
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["conversation_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec![conv.as_str()]);
}

#[sqlx::test]
async fn ending_waits_for_an_in_flight_ai_reply(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    student_says(&app, &a, &conv, "想法").await;

    sqlx::query("UPDATE conversations SET generating_since = now()")
        .execute(&pool)
        .await
        .unwrap();
    let (st, body) = end(&app, &a.student, &conv).await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "reply_in_progress");
    let d = json_body(
        send(
            &app,
            get(&format!("/api/conversations/{conv}"), Some(&a.student)),
        )
        .await,
    )
    .await;
    assert_eq!(d["status"], "active");

    // 回覆完成（claim 釋放）後即可結束
    sqlx::query("UPDATE conversations SET generating_since = NULL")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(end(&app, &a.student, &conv).await.0, StatusCode::ACCEPTED);
}

#[sqlx::test]
async fn superseded_generation_cannot_overwrite_the_newer_one(pool: PgPool) {
    // 第一代卡住（Hang）；在它逾時前手動重試，第二代很快成功。
    // 第一代之後失敗收尾時，不能把已完成的結果改成 failed。
    // 第一代的首字逾時要遠大於手動重試的等待，否則它自己的自動重試會先拿走第二個腳本
    let config = Config {
        ai_first_token_timeout: Duration::from_millis(1500),
        ai_total_timeout: Duration::from_millis(4000),
        ..test_config()
    };
    let ai = FakeAi::with(vec![Hang, Reply(vec![GOOD])]);
    let app = test_app_with_config(pool.clone(), ai.clone(), config);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    student_says(&app, &a, &conv, "想法").await;
    end(&app, &a.student, &conv).await;
    // 等第一代真的拿到 Hang 腳本，重試的第二代才會拿到 Reply
    while ai.request_count() < 1 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    sqlx::query("UPDATE summaries SET started_at = now() - interval '1 hour'")
        .execute(&pool)
        .await
        .unwrap();
    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/summary/retry"),
            Some(&a.student),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    assert_eq!(
        settled_summary(&app, &a.student, &conv).await["status"],
        "ready"
    );

    // 等第一代的逾時與自動重試都結束
    tokio::time::sleep(Duration::from_millis(1700)).await;
    let s = settled_summary(&app, &a.student, &conv).await;
    assert_eq!(s["status"], "ready");
    assert_eq!(s["stance"], "會拉桿");
}

#[sqlx::test]
async fn orphaned_pending_summaries_are_failed_on_startup(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    student_says(&app, &a, &conv, "想法").await;
    sqlx::query("UPDATE conversations SET status = 'ended'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO summaries (conversation_id) VALUES ($1::uuid)")
        .bind(&conv)
        .execute(&pool)
        .await
        .unwrap();
    let n = socrates_chat_backend::summary::recover_orphaned(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
    assert_eq!(
        settled_summary(&app, &a.student, &conv).await["status"],
        "failed"
    );
}
