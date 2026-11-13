//! F-29.3 活動完成條件與學生總結列（Issue #388、S-04.7）。
//!
//! 學生完成活動內的全部題目才算完成活動：
//! - 活動有 3 題，學生完成 2 題 -> 尚未完成（activity_completed: false）
//! - 學生完成全部 3 題 -> 已完成（activity_completed: true）
//! - 活動新增題目 -> 已完成其他題目的學生回到尚未完成（activity_completed: false）
//! - 學生刪除對話 -> 完成狀態即時更新
//! - 授權：教師只能查看自己教室的總結與已結束活動對話原文；個人練習、進行中對話與其他教室皆 404。

mod common;

use axum::{Router, http::StatusCode};
use common::Script::*;
use common::*;
use serde_json::json;
use sqlx::PgPool;
use std::collections::HashMap;

const GOOD: &str = r#"{"stance":"主張","reasons":"核心理由"}"#;

async fn add_teacher(app: &Router, admin: &str, sub: &str, email: &str) -> String {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(admin),
            Some(serde_json::json!({"email": email})),
        ),
    )
    .await;
    assert!(res.status().is_success());
    login_as(app, sub, email).await
}

async fn end_conv(app: &Router, cookie: &str, conv: &str) {
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
}

/// 學生開始並結束活動內某題的對話，等總結產生完；回傳對話 id 與總結狀態（ready／failed）。
async fn finish_topic_dialogue(
    app: &Router,
    pool: &PgPool,
    student_cookie: &str,
    activity_id: &str,
    topic_id: &str,
) -> (String, String) {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/conversations",
            Some(student_cookie),
            Some(json!({
                "activity_id": activity_id,
                "topic_id": topic_id,
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let conv = json_body(res).await["id"].as_str().unwrap().to_owned();

    seed_student_message(pool, &conv, "這是學生的哲學論點與回答").await;
    end_conv(app, student_cookie, &conv).await;
    let s = settled_summary(app, student_cookie, &conv).await;
    let status = s["status"].as_str().unwrap().to_owned();
    (conv, status)
}

async fn complete_topic_dialogue(
    app: &Router,
    pool: &PgPool,
    student_cookie: &str,
    activity_id: &str,
    topic_id: &str,
) -> String {
    let (conv, status) =
        finish_topic_dialogue(app, pool, student_cookie, activity_id, topic_id).await;
    assert_eq!(status, "ready");
    conv
}

/// 教師查活動的總結列，回傳「對話 id -> activity_completed」。
async fn completed_by_conversation(
    app: &Router,
    teacher: &str,
    classroom: &str,
    activity: &str,
) -> HashMap<String, bool> {
    let res = send(
        app,
        get(
            &format!("/api/summaries?classroom_id={classroom}&activity={activity}"),
            Some(teacher),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    json_body(res).await["items"]
        .as_array()
        .expect("items array")
        .iter()
        .map(|i| {
            (
                i["conversation_id"].as_str().unwrap().to_owned(),
                i["activity_completed"].as_bool().unwrap(),
            )
        })
        .collect()
}

#[sqlx::test]
async fn activity_completion_lifecycle_and_summary_rows(pool: PgPool) {
    // 準備 FakeAi 支援多次總結
    let ai = FakeAi::with(vec![
        Reply(vec![GOOD]),
        Reply(vec![GOOD]),
        Reply(vec![GOOD]),
        Reply(vec![GOOD]),
    ]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let a = seed_actors(&app).await;

    // 建立 3 個題目的活動
    let t1 = seed_topic("第一題：電車問題").await;
    let t2 = seed_topic("第二題：無知之幕").await;
    let t3 = seed_topic("第三題：洞穴寓言").await;

    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({
                "title": "多題目倫理學活動",
                "classroom_id": a.classroom,
                "topic_ids": [&t1, &t2, &t3]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let activity = json_body(res).await["id"].as_str().unwrap().to_owned();

    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/activities/{activity}/publish"),
            Some(&a.teacher),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    // 情境 1: 學生完成 2 題（t1, t2），活動有 3 題 -> 尚未完成（activity_completed: false）
    let c1 = complete_topic_dialogue(&app, &pool, &a.student, &activity, &t1).await;
    let c2 = complete_topic_dialogue(&app, &pool, &a.student, &activity, &t2).await;

    let res = send(
        &app,
        get(
            &format!(
                "/api/summaries?classroom_id={}&activity={}",
                a.classroom, activity
            ),
            Some(&a.teacher),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 2, "2 completed topics should show 2 rows");

    for item in items {
        assert_eq!(item["activity_id"], activity);
        assert_eq!(
            item["activity_completed"], false,
            "only completed 2 of 3 topics, so activity_completed must be false"
        );
        let conv_id = item["conversation_id"].as_str().unwrap();
        assert!(conv_id == c1 || conv_id == c2);
        let top_id = item["topic_id"].as_str().unwrap();
        assert!(top_id == t1 || top_id == t2);
    }

    // 情境 2: 學生完成第 3 題（t3） -> 已完成（activity_completed: true）
    let c3 = complete_topic_dialogue(&app, &pool, &a.student, &activity, &t3).await;

    let res = send(
        &app,
        get(
            &format!(
                "/api/summaries?classroom_id={}&activity={}",
                a.classroom, activity
            ),
            Some(&a.teacher),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 3, "3 completed topics show 3 rows");

    for item in items {
        assert_eq!(item["activity_id"], activity);
        assert_eq!(
            item["activity_completed"], true,
            "completed all 3 topics, so activity_completed must be true"
        );
        let conv_id = item["conversation_id"].as_str().unwrap();
        assert!(conv_id == c1 || conv_id == c2 || conv_id == c3);
    }

    // 情境 3: 活動新增第 4 題 -> 原已完成的學生回到尚未完成（activity_completed: false）
    let t4 = seed_topic("第四題：忒修斯之船").await;
    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/activities/{activity}"),
            Some(&a.teacher),
            Some(json!({
                "topic_ids": [&t1, &t2, &t3, &t4]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    let res = send(
        &app,
        get(
            &format!(
                "/api/summaries?classroom_id={}&activity={}",
                a.classroom, activity
            ),
            Some(&a.teacher),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 3);
    for item in items {
        assert_eq!(
            item["activity_completed"], false,
            "new topic was added, student now has 3/4 completed, so activity_completed must revert to false"
        );
    }

    // 情境 4: 學生刪除第 3 題對話 -> 只剩 2 題總結，依然尚未完成
    let res = send(
        &app,
        json_req(
            "DELETE",
            &format!("/api/conversations/{c3}"),
            Some(&a.student),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let res = send(
        &app,
        get(
            &format!(
                "/api/summaries?classroom_id={}&activity={}",
                a.classroom, activity
            ),
            Some(&a.teacher),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 2);
    for item in items {
        assert_eq!(item["activity_completed"], false);
    }
}

/// 完成條件只看「這位學生自己」「已完成（ready）」的總結：
/// 總結失敗的題目不算，其他學生完成同一題也不能替他補上；重新產生成功後才算。
#[sqlx::test]
async fn activity_completion_counts_only_own_ready_summaries(pool: PgPool) {
    // 呼叫順序：A 第 1 題 ready、A 第 2 題 failed（自動重試 1 次，所以要兩個 Fail，S-03.5）、
    // B 第 1 題 ready、B 第 2 題 ready、A 重新產生 ready
    let ai = FakeAi::with(vec![
        Reply(vec![GOOD]),
        Fail,
        Fail,
        Reply(vec![GOOD]),
        Reply(vec![GOOD]),
        Reply(vec![GOOD]),
    ]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let a = seed_actors(&app).await;
    let student_b = add_student(&app, &a, "sub-student-b", "student-b@example.com").await;

    let t1 = seed_topic("第一題：電車問題").await;
    let t2 = seed_topic("第二題：無知之幕").await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({
                "title": "兩題活動",
                "classroom_id": a.classroom,
                "topic_ids": [&t1, &t2]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let activity = json_body(res).await["id"].as_str().unwrap().to_owned();
    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/activities/{activity}/publish"),
            Some(&a.teacher),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    // A：第 1 題 ready、第 2 題總結失敗 -> A 沒完成；失敗的那場不會出現在總結列
    let (a1, st) = finish_topic_dialogue(&app, &pool, &a.student, &activity, &t1).await;
    assert_eq!(st, "ready");
    let (a2, st) = finish_topic_dialogue(&app, &pool, &a.student, &activity, &t2).await;
    assert_eq!(st, "failed");

    let rows = completed_by_conversation(&app, &a.teacher, &a.classroom, &activity).await;
    assert_eq!(rows.len(), 1, "failed summary is not listed");
    assert!(
        !rows[&a1],
        "a failed summary must not count as a completed topic"
    );

    // B 完成兩題 -> B 完成；A 不會因為 B 完成了同一個活動而跟著完成
    let b1 = complete_topic_dialogue(&app, &pool, &student_b, &activity, &t1).await;
    let b2 = complete_topic_dialogue(&app, &pool, &student_b, &activity, &t2).await;

    let rows = completed_by_conversation(&app, &a.teacher, &a.classroom, &activity).await;
    assert_eq!(rows.len(), 3);
    assert!(rows[&b1]);
    assert!(rows[&b2]);
    assert!(
        !rows[&a1],
        "another student's completed topics must not complete mine"
    );

    // A 重新產生失敗的總結並成功 -> A 完成
    assert_eq!(
        retry_summary(&app, &a.student, &a2).await,
        StatusCode::ACCEPTED
    );
    assert_eq!(
        settled_summary(&app, &a.student, &a2).await["status"],
        "ready"
    );

    let rows = completed_by_conversation(&app, &a.teacher, &a.classroom, &activity).await;
    assert_eq!(rows.len(), 4);
    assert!(rows[&a1]);
    assert!(rows[&a2]);
    assert!(rows[&b1]);
    assert!(rows[&b2]);
}

#[sqlx::test]
async fn activity_completion_authorization_boundaries(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD]), Reply(vec![GOOD])]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let a = seed_actors(&app).await;

    let other_teacher = add_teacher(
        &app,
        &a.admin,
        "sub-other-teacher",
        "other_teacher@example.com",
    )
    .await;
    let other_room = create_classroom(&app, &other_teacher, "其他教師教室").await;

    // 建立 1 題活動並完成
    let t1 = seed_topic("單一題目：正義論").await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({
                "title": "單題目討論活動",
                "classroom_id": a.classroom,
                "topic_ids": [&t1]
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

    let conv = complete_topic_dialogue(&app, &pool, &a.student, &activity, &t1).await;

    // 1. 本教室教師查詢自己的教室總結 -> 200，單題目完成時 activity_completed 為 true
    let res = send(
        &app,
        get(
            &format!(
                "/api/summaries?classroom_id={}&activity={}",
                a.classroom, activity
            ),
            Some(&a.teacher),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["activity_completed"], true);
    assert_eq!(items[0]["activity_id"], activity);
    assert_eq!(items[0]["topic_id"], t1);

    // 2. 他人教師查詢該教室 -> 404
    let res = send(
        &app,
        get(
            &format!(
                "/api/summaries?classroom_id={}&activity={}",
                a.classroom, activity
            ),
            Some(&other_teacher),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // 他人教師查自己的教室 -> 200 但為空（不外露其他教室資料）
    let res = send(
        &app,
        get(
            &format!("/api/summaries?classroom_id={}", other_room),
            Some(&other_teacher),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_body(res).await["items"].as_array().unwrap().len(), 0);

    // 3. 學生與名單外帳號呼叫總結列表 -> 403
    let res = send(
        &app,
        get(
            &format!("/api/summaries?classroom_id={}", a.classroom),
            Some(&a.student),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    let res = send(
        &app,
        get(
            &format!("/api/summaries?classroom_id={}", a.classroom),
            Some(&a.outsider),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 4. 查看對話原文 GET /api/summaries/{id}/messages
    // 本教室教師 -> 200，含完整訊息
    let res = send(
        &app,
        get(&format!("/api/summaries/{conv}/messages"), Some(&a.teacher)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let trans = json_body(res).await;
    assert_eq!(trans["conversation_id"], conv);
    assert_eq!(trans["messages"].as_array().unwrap().len(), 1);
    assert_eq!(trans["messages"][0]["content"], "這是學生的哲學論點與回答");

    // 管理者 -> 200，但內容遮蔽為 message
    let res = send(
        &app,
        get(&format!("/api/summaries/{conv}/messages"), Some(&a.admin)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let trans = json_body(res).await;
    assert_eq!(trans["messages"][0]["content"], "message");

    // 他人教師 -> 404
    let res = send(
        &app,
        get(
            &format!("/api/summaries/{conv}/messages"),
            Some(&other_teacher),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // 學生與名單外使用者 -> 403
    let res = send(
        &app,
        get(&format!("/api/summaries/{conv}/messages"), Some(&a.student)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 5. 進行中對話不能透過 /api/summaries/{id}/messages 查看 -> 404
    // (因唯一索引，先刪除舊對話重開進行中對話)
    send(
        &app,
        json_req(
            "DELETE",
            &format!("/api/conversations/{conv}"),
            Some(&a.student),
            None,
        ),
    )
    .await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/conversations",
            Some(&a.student),
            Some(json!({
                "activity_id": activity,
                "topic_id": t1,
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let active_conv = json_body(res).await["id"].as_str().unwrap().to_owned();

    let res = send(
        &app,
        get(
            &format!("/api/summaries/{active_conv}/messages"),
            Some(&a.teacher),
        ),
    )
    .await;
    assert_eq!(
        res.status(),
        StatusCode::NOT_FOUND,
        "active conversation messages must return 404 in teacher summary view"
    );

    // 6. 個人練習對話不能透過 /api/summaries/{id}/messages 查看 -> 404
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/conversations",
            Some(&a.student),
            Some(json!({
                "topic_id": t1,
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let practice_conv = json_body(res).await["id"].as_str().unwrap().to_owned();
    seed_student_message(&pool, &practice_conv, "個人練習內容").await;
    end_conv(&app, &a.student, &practice_conv).await;

    let res = send(
        &app,
        get(
            &format!("/api/summaries/{practice_conv}/messages"),
            Some(&a.teacher),
        ),
    )
    .await;
    assert_eq!(
        res.status(),
        StatusCode::NOT_FOUND,
        "personal practice messages must return 404"
    );
}
