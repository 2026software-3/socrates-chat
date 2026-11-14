//! F-24.1-BE（#567）單場雷達圖：自動計算、狀態查詢、手動重試與授權（假 AI、合成資料）。

mod common;

use std::{sync::Arc, time::Duration};

use axum::{Router, http::StatusCode};
use common::Script::*;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const GOOD: &str = "{\"stance\":\"會拉桿\",\"reasons\":\"救五人優先\"}";
const SCORES: &str = r#"{"d1":5,"d2":0,"d3":3,"d4":1,"d5":0,"d6":2}"#;
const OTHER_SCORES: &str = r#"{"d1":0,"d2":4,"d3":1,"d4":0,"d5":0,"d6":0}"#;
const MESSAGE: &str = "我會拉桿，因為五個人比一個人多。";

/// 直接寫入題目的六個維度，不呼叫 AI。
async fn seed_dimensions(pool: &PgPool, topic: &str) {
    let dimensions: Vec<Value> = (1..=6)
        .map(|n| {
            json!({
                "id": format!("d{n}"),
                "name": {"zh_tw": format!("傾向{n}"), "en": format!("Tendency {n}"), "es": format!("Tendencia {n}")},
                "description": {"zh_tw": format!("說明{n}"), "en": format!("About {n}"), "es": format!("Sobre {n}")},
                "rubric": (1..=5).map(|level| format!("傾向{n}的第{level}級")).collect::<Vec<_>>(),
            })
        })
        .collect();
    sqlx::query(
        "UPDATE topics SET dimensions = $2, dimensions_prompt_version = 'topic-dimensions-v1',
                dimensions_language = 'zh-TW' WHERE id = $1",
    )
    .bind(topic.parse::<Uuid>().unwrap())
    .bind(sqlx::types::Json(dimensions))
    .execute(pool)
    .await
    .unwrap();
}

struct Fixture {
    app: Router,
    ai: Arc<FakeAi>,
    actors: Actors,
    activity: String,
    topic: String,
    conversation: String,
}

/// 學生在一個活動裡有一場進行中、已有一則訊息的對話。
async fn fixture(pool: &PgPool, scripts: Vec<Script>, with_dimensions: bool) -> Fixture {
    let ai = FakeAi::with(scripts);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let actors = seed_actors(&app).await;
    let topic = seed_topic("電車難題").await;
    if with_dimensions {
        seed_dimensions(pool, &topic).await;
    }
    let activity = publish_activity(&app, &actors, "電車難題", &topic).await;
    let conversation = start_from_activity(&app, &actors.student, &activity).await;
    seed_student_message(pool, &conversation, MESSAGE).await;
    Fixture {
        app,
        ai,
        actors,
        activity,
        topic,
        conversation,
    }
}

async fn end(app: &Router, cookie: &str, conversation: &str) {
    let res = send(
        app,
        json_req(
            "POST",
            &format!("/api/conversations/{conversation}/end"),
            Some(cookie),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    settled_summary(app, cookie, conversation).await;
}

fn radar_path(conversation: &str) -> String {
    format!("/api/conversations/{conversation}/radar")
}

async fn radar(app: &Router, cookie: &str, conversation: &str) -> Value {
    let res = send(app, get(&radar_path(conversation), Some(cookie))).await;
    assert_eq!(res.status(), StatusCode::OK);
    json_body(res).await
}

/// 等到雷達圖進入 `status`；逾時就讓測試失敗。
async fn radar_until(app: &Router, cookie: &str, conversation: &str, status: &str) -> Value {
    let mut last = Value::Null;
    for _ in 0..400 {
        last = radar(app, cookie, conversation).await;
        if last["status"] == status {
            return last;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!(
        "radar never became {status}; last status was {}",
        last["status"]
    );
}

async fn retry(app: &Router, cookie: Option<&str>, conversation: &str) -> axum::response::Response {
    send(
        app,
        json_req(
            "POST",
            &format!("{}/retry", radar_path(conversation)),
            cookie,
            None,
        ),
    )
    .await
}

/// 讓「總結之後不該再有 AI 呼叫」的背景任務有時間跑完。
async fn let_background_settle() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}

#[sqlx::test]
async fn a_ready_summary_scores_the_conversation_on_its_topic_dimensions(pool: PgPool) {
    let f = fixture(&pool, vec![Reply(vec![GOOD]), Reply(vec![SCORES])], true).await;
    end(&f.app, &f.actors.student, &f.conversation).await;

    // 總結一 ready，計算就已登記：不會出現「尚未計算」的空窗
    let registered = radar(&f.app, &f.actors.student, &f.conversation).await;
    assert!(
        registered["status"] == "pending" || registered["status"] == "ready",
        "unexpected status {}",
        registered["status"]
    );
    let body = radar_until(&f.app, &f.actors.student, &f.conversation, "ready").await;
    assert_eq!(
        body["scores"],
        json!({"d1": 5, "d2": 0, "d3": 3, "d4": 1, "d5": 0, "d6": 2})
    );
    assert_eq!(body["retryable"], false);
    assert_eq!(body["rules_version"], "radar-scoring-v1");
    assert!(body["completed_at"].is_string());
    let dimensions = body["dimensions"].as_array().unwrap();
    assert_eq!(dimensions.len(), 6);
    assert_eq!(dimensions[0]["id"], "d1");
    assert_eq!(dimensions[0]["name"]["zh_tw"], "傾向1");
    assert_eq!(dimensions[5]["description"]["es"], "Sobre 6");
    assert!(
        dimensions[0].get("rubric").is_none(),
        "the scoring rubric is not shown to students"
    );

    // 第二次 AI 呼叫是評分：只帶這場對話與題目自己的維度與準則
    assert_eq!(f.ai.request_count(), 2);
    {
        let requests = f.ai.requests.lock().unwrap();
        assert!(requests[1].system.starts_with("radar-scoring-v1"));
        let input = &requests[1].messages[0].content;
        assert!(input.contains(MESSAGE));
        assert!(input.contains("傾向1的第1級"));
    }

    // 分數寫入後判定「任一維度達分數」成就（S-07.3）：d1 為 5
    let unlocks: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM achievement_unlocks u JOIN users ON users.id = u.user_id
         WHERE email = 'student@example.com' AND achievement_id = 'sharp_reasoner'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(unlocks, 1);
}

#[sqlx::test]
async fn a_failed_scoring_keeps_the_summary_and_can_be_retried_once(pool: PgPool) {
    // 評分第一次失敗、自動重試回傳缺漏的分數：兩次都不採用
    let f = fixture(
        &pool,
        vec![
            Reply(vec![GOOD]),
            Fail,
            Reply(vec![r#"{"d1":3}"#]),
            Reply(vec![SCORES]),
        ],
        true,
    )
    .await;
    end(&f.app, &f.actors.student, &f.conversation).await;

    let failed = radar_until(&f.app, &f.actors.student, &f.conversation, "failed").await;
    assert_eq!(failed["retryable"], true);
    assert!(failed["scores"].is_null(), "no partial scores");
    assert!(failed["rules_version"].is_null());
    assert_eq!(f.ai.request_count(), 3);
    let summary = settled_summary(&f.app, &f.actors.student, &f.conversation).await;
    assert_eq!(summary["status"], "ready");
    assert_eq!(summary["stance"], "會拉桿");

    let res = retry(&f.app, Some(&f.actors.student), &f.conversation).await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let accepted = json_body(res).await;
    assert_eq!(accepted["status"], "pending");
    assert_eq!(accepted["retryable"], false);
    let ready = radar_until(&f.app, &f.actors.student, &f.conversation, "ready").await;
    assert_eq!(ready["scores"]["d1"], 5);
    assert_eq!(f.ai.request_count(), 4);

    // 已成功就不能再觸發，也不再呼叫 AI
    let res = retry(&f.app, Some(&f.actors.student), &f.conversation).await;
    assert_json_error(res, StatusCode::CONFLICT, "radar_not_retryable").await;
    assert_eq!(f.ai.request_count(), 4);
}

#[sqlx::test]
async fn scoring_waits_for_topic_dimensions_and_can_be_triggered_once_they_exist(pool: PgPool) {
    let f = fixture(&pool, vec![Reply(vec![GOOD]), Reply(vec![SCORES])], false).await;
    end(&f.app, &f.actors.student, &f.conversation).await;
    let_background_settle().await;

    let waiting = radar(&f.app, &f.actors.student, &f.conversation).await;
    assert_eq!(waiting["status"], "dimensions_pending");
    assert_eq!(waiting["retryable"], false);
    assert!(waiting["dimensions"].is_null());
    assert_eq!(f.ai.request_count(), 1, "only the summary called the AI");
    let res = retry(&f.app, Some(&f.actors.student), &f.conversation).await;
    assert_json_error(res, StatusCode::CONFLICT, "dimensions_pending").await;

    seed_dimensions(&pool, &f.topic).await;
    let startable = radar(&f.app, &f.actors.student, &f.conversation).await;
    assert_eq!(startable["status"], "not_started");
    assert_eq!(startable["retryable"], true);
    assert_eq!(startable["dimensions"].as_array().unwrap().len(), 6);

    let res = retry(&f.app, Some(&f.actors.student), &f.conversation).await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let ready = radar_until(&f.app, &f.actors.student, &f.conversation, "ready").await;
    assert_eq!(ready["scores"]["d3"], 3);
}

#[sqlx::test]
async fn nothing_is_scored_before_the_summary_is_ready(pool: PgPool) {
    // 總結的兩次嘗試都失敗
    let f = fixture(&pool, vec![Fail, Fail, Reply(vec![SCORES])], true).await;
    let active = radar(&f.app, &f.actors.student, &f.conversation).await;
    assert_eq!(active["status"], "summary_not_ready");
    assert_eq!(active["retryable"], false);
    let res = retry(&f.app, Some(&f.actors.student), &f.conversation).await;
    assert_json_error(res, StatusCode::CONFLICT, "radar_not_retryable").await;

    end(&f.app, &f.actors.student, &f.conversation).await;
    let_background_settle().await;
    let failed_summary = radar(&f.app, &f.actors.student, &f.conversation).await;
    assert_eq!(failed_summary["status"], "summary_not_ready");
    let res = retry(&f.app, Some(&f.actors.student), &f.conversation).await;
    assert_json_error(res, StatusCode::CONFLICT, "radar_not_retryable").await;
    assert_eq!(f.ai.request_count(), 2, "no scoring call was made");
}

#[sqlx::test]
async fn a_running_calculation_cannot_be_restarted_until_it_is_stale(pool: PgPool) {
    let f = fixture(&pool, vec![Reply(vec![GOOD]), Reply(vec![SCORES])], false).await;
    end(&f.app, &f.actors.student, &f.conversation).await;
    let_background_settle().await;
    seed_dimensions(&pool, &f.topic).await;
    let id: Uuid = f.conversation.parse().unwrap();
    // 模擬另一個工作正在計算
    sqlx::query("INSERT INTO conversation_radar_results (conversation_id) VALUES ($1)")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();

    let running = radar(&f.app, &f.actors.student, &f.conversation).await;
    assert_eq!(running["status"], "pending");
    assert_eq!(running["retryable"], false);
    let res = retry(&f.app, Some(&f.actors.student), &f.conversation).await;
    assert_json_error(res, StatusCode::CONFLICT, "radar_not_retryable").await;
    assert_eq!(f.ai.request_count(), 1);

    // 超過逾時上限（例如服務重啟）後可以接手
    sqlx::query(
        "UPDATE conversation_radar_results SET started_at = now() - interval '1 hour'
         WHERE conversation_id = $1",
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();
    let stuck = radar(&f.app, &f.actors.student, &f.conversation).await;
    assert_eq!(stuck["status"], "pending");
    assert_eq!(stuck["retryable"], true);
    let res = retry(&f.app, Some(&f.actors.student), &f.conversation).await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    radar_until(&f.app, &f.actors.student, &f.conversation, "ready").await;
    let attempt: i32 = sqlx::query_scalar(
        "SELECT attempt FROM conversation_radar_results WHERE conversation_id = $1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(attempt, 2);
}

#[sqlx::test]
async fn each_conversation_is_scored_only_on_its_own_messages(pool: PgPool) {
    let f = fixture(
        &pool,
        vec![
            Reply(vec![GOOD]),
            Reply(vec![SCORES]),
            Reply(vec![GOOD]),
            Reply(vec![OTHER_SCORES]),
        ],
        true,
    )
    .await;
    let other = add_student(&f.app, &f.actors, "sub-other", "other@example.com").await;
    let other_conversation = start_from_activity(&f.app, &other, &f.activity).await;
    let other_message = "我不會拉桿，主動害死一個人是錯的。";
    seed_student_message(&pool, &other_conversation, other_message).await;

    end(&f.app, &f.actors.student, &f.conversation).await;
    let first = radar_until(&f.app, &f.actors.student, &f.conversation, "ready").await;
    end(&f.app, &other, &other_conversation).await;
    let second = radar_until(&f.app, &other, &other_conversation, "ready").await;

    assert_eq!(first["scores"]["d1"], 5);
    assert_eq!(second["scores"]["d1"], 0);
    assert_eq!(second["scores"]["d2"], 4);
    let requests = f.ai.requests.lock().unwrap();
    let (first_input, second_input) = (
        &requests[1].messages[0].content,
        &requests[3].messages[0].content,
    );
    assert!(first_input.contains(MESSAGE) && !first_input.contains(other_message));
    assert!(second_input.contains(other_message) && !second_input.contains(MESSAGE));
}

#[sqlx::test]
async fn only_the_enrolled_owner_can_read_or_trigger_a_radar(pool: PgPool) {
    let f = fixture(&pool, vec![Reply(vec![GOOD]), Fail, Fail], true).await;
    end(&f.app, &f.actors.student, &f.conversation).await;
    radar_until(&f.app, &f.actors.student, &f.conversation, "failed").await;
    let other = add_student(&f.app, &f.actors, "sub-other", "other@example.com").await;

    for (cookie, status, code) in [
        (None, StatusCode::UNAUTHORIZED, "unauthorized"),
        (Some(&f.actors.teacher), StatusCode::FORBIDDEN, "forbidden"),
        (Some(&f.actors.admin), StatusCode::FORBIDDEN, "forbidden"),
        (
            Some(&f.actors.outsider),
            StatusCode::FORBIDDEN,
            "not_enrolled",
        ),
        (Some(&other), StatusCode::NOT_FOUND, "not_found"),
    ] {
        let cookie = cookie.map(String::as_str);
        let res = send(&f.app, get(&radar_path(&f.conversation), cookie)).await;
        assert_json_error(res, status, code).await;
        let res = retry(&f.app, cookie, &f.conversation).await;
        assert_json_error(res, status, code).await;
    }
    // 被拒絕的請求沒有觸發任何計算
    let still = radar(&f.app, &f.actors.student, &f.conversation).await;
    assert_eq!(still["status"], "failed");
    assert_eq!(f.ai.request_count(), 3);
}

#[sqlx::test]
async fn an_archived_classroom_is_read_only_for_radar_calculation(pool: PgPool) {
    let f = fixture(&pool, vec![Reply(vec![GOOD]), Fail, Fail], true).await;
    end(&f.app, &f.actors.student, &f.conversation).await;
    radar_until(&f.app, &f.actors.student, &f.conversation, "failed").await;
    let res = send(
        &f.app,
        json_req(
            "PATCH",
            &format!("/api/classrooms/{}", f.actors.classroom),
            Some(&f.actors.teacher),
            Some(json!({"is_archived": true})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    let res = retry(&f.app, Some(&f.actors.student), &f.conversation).await;
    assert_json_error(res, StatusCode::CONFLICT, "classroom_archived").await;
    // 仍可查看狀態，且沒有呼叫 AI
    let still = radar(&f.app, &f.actors.student, &f.conversation).await;
    assert_eq!(still["status"], "failed");
    assert_eq!(f.ai.request_count(), 3);
}
