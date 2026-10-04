//! 主張分組：AI 為每場對話寫一句主張，同題目下把相近的歸成一群（暫定，對應 S-04）。
//! 假 AI、合成資料；送給 AI 的只有主張短句與題目標題。

mod common;

use std::{sync::Arc, time::Duration};

use axum::{Router, http::StatusCode};
use common::Script::*;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

fn summary(claim: &str, reasons: &str, utilitarianism: u8) -> String {
    json!({
        "stance": "示範立場", "reasons": reasons, "claim": claim,
        "framework": "utilitarianism",
        "scores": {"utilitarianism": utilitarianism, "deontology": 0, "virtue": 0,
                   "contractarianism": 0, "care": 0, "existentialism": 0},
    })
    .to_string()
}

async fn wait_for_requests(ai: &Arc<FakeAi>, n: usize) {
    for _ in 0..200 {
        if ai.request_count() >= n {
            // 再給背景任務一點時間把結果寫進資料庫
            tokio::time::sleep(Duration::from_millis(100)).await;
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("AI was not called {n} times");
}

/// 學生開始並結束一場對話（題目標題為「電車難題」），等 `n` 次 AI 呼叫都完成。
async fn finish(app: &Router, a: &Actors, ai: &Arc<FakeAi>, n: usize) {
    let conv = start_conversation(app, a).await;
    student_says(app, a, &conv, "我的想法").await;
    send(
        app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/end"),
            Some(&a.student),
            None,
        ),
    )
    .await;
    wait_for_requests(ai, n).await;
}

async fn class(app: &Router, cookie: &str) -> Value {
    json_body(send(app, get("/api/dashboard/class", Some(cookie))).await).await
}

async fn regroup(app: &Router, cookie: Option<&str>) -> (StatusCode, Value) {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/dashboard/class/regroup",
            cookie,
            Some(json!({"title": "電車難題"})),
        ),
    )
    .await;
    (res.status(), json_body(res).await)
}

#[sqlx::test]
async fn similar_claims_share_a_group_and_the_second_call_sees_existing_names(pool: PgPool) {
    let s1 = summary("該拉桿", "救五人比救一人重要", 5);
    let s2 = summary("應該拉桿", "結果最重要", 3);
    let ai = FakeAi::with(vec![
        Reply(vec![Box::leak(s1.into_boxed_str())]),
        Reply(vec!["{\"group\":\"拉桿\"}"]),
        Reply(vec![Box::leak(s2.into_boxed_str())]),
        Reply(vec!["{\"group\":\" 拉桿 \"}"]),
    ]);
    let app = test_app_with_ai(pool, ai.clone());
    let a = seed_actors(&app).await;
    finish(&app, &a, &ai, 2).await;
    finish(&app, &a, &ai, 4).await;

    let d = class(&app, &a.teacher).await;
    let topic = &d["topics"][0];
    assert_eq!(topic["title"], "電車難題");
    let claims = topic["claims"].as_array().unwrap();
    assert_eq!(claims.len(), 1, "意思相近的主張歸成同一群");
    assert_eq!(claims[0]["name"], "拉桿");
    assert_eq!(claims[0]["count"], 2);
    assert_eq!(claims[0]["other"], false);
    let reasons = claims[0]["reasons"].as_array().unwrap();
    assert!(
        reasons.contains(&json!("救五人比救一人重要")) && reasons.contains(&json!("結果最重要"))
    );
    // 群組內的 6 維平均：效益主義 (5 + 3) / 2
    assert_eq!(claims[0]["radar"]["scored"], 2);
    assert_eq!(claims[0]["radar"]["average"]["utilitarianism"], 4.0);
    assert_eq!(topic["ungrouped"], 0);

    // 第二次歸群時，既有群組名稱有一併交給 AI；只送主張短句與題目，沒有對話原文
    let reqs = ai.requests.lock().unwrap();
    let assign = &reqs[3];
    assert!(assign.system.contains("- 拉桿"));
    assert_eq!(assign.messages[0].content, "應該拉桿");
    assert!(
        !assign.system.contains("我的想法") && !assign.messages[0].content.contains("我的想法")
    );
}

#[sqlx::test]
async fn failed_grouping_leaves_the_claim_ungrouped_until_regroup(pool: PgPool) {
    let s1 = summary("該拉桿", "理由甲", 4);
    let ai = FakeAi::with(vec![
        Reply(vec![Box::leak(s1.into_boxed_str())]),
        Fail, // 歸群失敗
        Reply(vec!["{\"groups\":[{\"name\":\"拉桿\",\"members\":[1]}]}"]),
    ]);
    let app = test_app_with_ai(pool, ai.clone());
    let a = seed_actors(&app).await;
    finish(&app, &a, &ai, 2).await;

    // 總結不受影響，只是這場暫時沒有群組
    let d = class(&app, &a.teacher).await;
    assert_eq!(d["completed"], 1);
    assert!(d["topics"][0]["claims"].as_array().unwrap().is_empty());
    assert_eq!(d["topics"][0]["ungrouped"], 1);

    let (status, _) = regroup(&app, Some(&a.teacher)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let d = class(&app, &a.teacher).await;
    assert_eq!(d["topics"][0]["claims"][0]["name"], "拉桿");
    assert_eq!(d["topics"][0]["ungrouped"], 0);
    // 重新分組送給 AI 的是編號的主張短句
    let reqs = ai.requests.lock().unwrap();
    assert!(reqs[2].system.contains("1. 該拉桿"));
}

#[sqlx::test]
async fn regroup_is_for_teachers_and_keeps_old_groups_when_the_ai_fails(pool: PgPool) {
    let s1 = summary("該拉桿", "理由甲", 4);
    let ai = FakeAi::with(vec![
        Reply(vec![Box::leak(s1.into_boxed_str())]),
        Reply(vec!["{\"group\":\"拉桿\"}"]),
        Fail,
    ]);
    let app = test_app_with_ai(pool, ai.clone());
    let a = seed_actors(&app).await;
    finish(&app, &a, &ai, 2).await;

    for (cookie, status) in [
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        (Some(a.admin.as_str()), StatusCode::FORBIDDEN),
        (None, StatusCode::UNAUTHORIZED),
    ] {
        assert_eq!(regroup(&app, cookie).await.0, status);
    }
    assert_eq!(ai.request_count(), 2, "未授權的請求不能碰到 AI");

    let (status, body) = regroup(&app, Some(&a.teacher)).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body["error"]["code"], "ai_unavailable");
    // AI 失敗時沿用原本的分組
    assert_eq!(
        class(&app, &a.teacher).await["topics"][0]["claims"][0]["name"],
        "拉桿"
    );

    // 沒有主張的題目：回 409
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/dashboard/class/regroup",
            Some(&a.teacher),
            Some(json!({"title": "不存在的題目"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(res).await["error"]["code"], "no_claims");
}

#[sqlx::test]
async fn the_smallest_groups_are_merged_into_other_and_admins_see_no_claims(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (user,): (uuid::Uuid,) =
        sqlx::query_as("SELECT id FROM users WHERE email = 'student@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    // 8 個群組，人數 8、7、…、1：前 6 群獨立列出，其餘併成「其他」
    for n in 1..=8_i32 {
        let (gid,): (uuid::Uuid,) = sqlx::query_as(
            "INSERT INTO claim_groups (topic_title, name) VALUES ('電車難題', $1) RETURNING id",
        )
        .bind(format!("主張{n}"))
        .fetch_one(&pool)
        .await
        .unwrap();
        for _ in 0..n {
            sqlx::query(
                "WITH c AS (INSERT INTO conversations (user_id, title, status, ended_at)
                            VALUES ($1, '電車難題', 'ended', now()) RETURNING id)
                 INSERT INTO summaries (conversation_id, status, stance, reasons, claim,
                                        claim_group_id, completed_at)
                 SELECT id, 'ready', 's', $3, $4, $2, now() FROM c",
            )
            .bind(user)
            .bind(gid)
            .bind(format!("理由{n}"))
            .bind(format!("主張{n}"))
            .execute(&pool)
            .await
            .unwrap();
        }
    }
    let d = class(&app, &a.teacher).await;
    let claims = d["topics"][0]["claims"].as_array().unwrap();
    assert_eq!(claims.len(), 7);
    let names: Vec<&str> = claims[..6]
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["主張8", "主張7", "主張6", "主張5", "主張4", "主張3"]
    );
    assert_eq!(claims[6]["other"], true);
    assert!(claims[6]["name"].is_null());
    assert_eq!(claims[6]["count"], 2 + 1);
    assert_eq!(claims[0]["reasons"], json!(["理由8"]), "同樣的理由只列一次");

    // 管理者看不到分析結果
    let d = class(&app, &a.admin).await;
    assert_eq!(d["masked"], true);
    assert!(d["topics"].as_array().unwrap().is_empty());
}

#[sqlx::test]
async fn students_and_teachers_see_the_claim_but_admins_get_the_masked_text(pool: PgPool) {
    let s1 = summary("該拉桿", "理由甲", 4);
    let ai = FakeAi::with(vec![
        Reply(vec![Box::leak(s1.into_boxed_str())]),
        Reply(vec!["{\"group\":\"拉桿\"}"]),
    ]);
    let app = test_app_with_ai(pool, ai.clone());
    let a = seed_actors(&app).await;
    finish(&app, &a, &ai, 2).await;

    let list = json_body(send(&app, get("/api/teacher/summaries", Some(&a.teacher))).await).await;
    assert_eq!(list[0]["claim"], "該拉桿");
    let list = json_body(send(&app, get("/api/teacher/summaries", Some(&a.admin))).await).await;
    assert_eq!(list[0]["claim"], "message");
    let me = json_body(send(&app, get("/api/dashboard/me", Some(&a.student))).await).await;
    assert_eq!(me["recent"][0]["claim"], "該拉桿");
}
