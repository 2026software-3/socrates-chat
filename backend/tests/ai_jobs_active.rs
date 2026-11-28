//! `GET /api/ai-jobs/active`（S-20 §3.3）：只彙總自己的工作、不帶內容。全部使用合成資料。

mod common;

use axum::{Router, http::StatusCode};
use common::*;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

async fn active(app: &Router, cookie: Option<&str>) -> (StatusCode, Value) {
    let res = send(app, get("/api/ai-jobs/active", cookie)).await;
    (res.status(), json_body(res).await)
}

fn ids(body: &Value) -> Vec<String> {
    body["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| j["id"].as_str().unwrap().to_string())
        .collect()
}

async fn fixture(pool: &PgPool) -> (Router, Actors, Uuid) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let conv: Uuid = start_conversation(&app, &a).await.parse().unwrap();
    (app, a, conv)
}

async fn exec(pool: &PgPool, sql: &'static str, conv: Uuid) {
    sqlx::query(sql).bind(conv).execute(pool).await.unwrap();
}

#[sqlx::test]
async fn requires_login(pool: PgPool) {
    let app = test_app(pool);
    let res = send(&app, get("/api/ai-jobs/active", None)).await;
    assert_json_error(res, StatusCode::UNAUTHORIZED, "unauthorized").await;
}

#[sqlx::test]
async fn lists_own_pending_work_with_ids_only(pool: PgPool) {
    let (app, a, conv) = fixture(&pool).await;
    exec(
        &pool,
        "INSERT INTO summaries (conversation_id, status) VALUES ($1, 'pending')",
        conv,
    )
    .await;
    exec(
        &pool,
        "UPDATE conversations SET generating_since = now() WHERE id = $1",
        conv,
    )
    .await;

    let (status, body) = active(&app, Some(&a.student)).await;
    assert_eq!(status, StatusCode::OK);
    let mut got = ids(&body);
    got.sort();
    assert_eq!(
        got,
        vec![format!("reply:{conv}"), format!("summary:{conv}")]
    );
    let job = body["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["kind"] == "summary")
        .unwrap();
    assert_eq!(job["status"], "pending");
    assert_eq!(job["label_key"], "jobs.summary");
    assert_eq!(job["scope"]["conversation_id"], conv.to_string());
    assert_eq!(job["retryable"], false);
    assert!(job["finished_at"].is_null());
    // 只有識別碼與狀態：不帶題目、訊息或總結內容
    let mut keys: Vec<_> = job.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "finished_at",
            "id",
            "kind",
            "label_key",
            "retryable",
            "scope",
            "started_at",
            "status"
        ]
    );
    assert_eq!(job["scope"].as_object().unwrap().len(), 1);
}

#[sqlx::test]
async fn never_shows_other_peoples_work(pool: PgPool) {
    let (app, a, conv) = fixture(&pool).await;
    exec(
        &pool,
        "INSERT INTO summaries (conversation_id, status) VALUES ($1, 'pending')",
        conv,
    )
    .await;
    exec(
        &pool,
        "UPDATE conversations SET generating_since = now() WHERE id = $1",
        conv,
    )
    .await;
    for cookie in [&a.outsider, &a.teacher, &a.admin] {
        let (status, body) = active(&app, Some(cookie)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(ids(&body).is_empty(), "{body}");
    }
}

#[sqlx::test]
async fn finished_work_is_listed_only_for_ten_minutes_and_failed_can_be_retried(pool: PgPool) {
    let (app, a, conv) = fixture(&pool).await;
    exec(
        &pool,
        "INSERT INTO summaries (conversation_id, status, started_at) VALUES ($1, 'failed', now() - interval '1 minute')",
        conv,
    )
    .await;
    let (_, body) = active(&app, Some(&a.student)).await;
    assert_eq!(body["jobs"][0]["status"], "failed");
    assert_eq!(body["jobs"][0]["retryable"], true);

    exec(
        &pool,
        "UPDATE summaries SET started_at = now() - interval '11 minutes' WHERE conversation_id = $1",
        conv,
    )
    .await;
    assert!(ids(&active(&app, Some(&a.student)).await.1).is_empty());
}

#[sqlx::test]
async fn long_running_pending_work_stays_listed_but_a_stale_reply_marker_does_not(pool: PgPool) {
    let (app, a, conv) = fixture(&pool).await;
    exec(
        &pool,
        "INSERT INTO summaries (conversation_id, status, started_at) VALUES ($1, 'pending', now() - interval '2 hours')",
        conv,
    )
    .await;
    exec(
        &pool,
        "UPDATE conversations SET generating_since = now() - interval '2 hours' WHERE id = $1",
        conv,
    )
    .await;
    assert_eq!(
        ids(&active(&app, Some(&a.student)).await.1),
        vec![format!("summary:{conv}")]
    );
}

#[sqlx::test]
async fn includes_radar_argument_tree_and_pending_translation(pool: PgPool) {
    let (app, a, conv) = fixture(&pool).await;
    // 對話語言 zh-TW、帳號語言 en：總結完成但還沒有英文翻譯
    sqlx::query("UPDATE users SET language = 'en' WHERE email = 'student@example.com'")
        .execute(&pool)
        .await
        .unwrap();
    exec(
        &pool,
        "INSERT INTO summaries (conversation_id, status, stance, reasons, completed_at)
         VALUES ($1, 'ready', 's', 'r', now())",
        conv,
    )
    .await;
    exec(
        &pool,
        "INSERT INTO conversation_radar_results (conversation_id, status) VALUES ($1, 'pending')",
        conv,
    )
    .await;
    exec(
        &pool,
        "INSERT INTO conversation_argument_trees (conversation_id, status, attempt, prompt_version)
         VALUES ($1, 'pending', 1, 'v')",
        conv,
    )
    .await;
    let mut got = ids(&active(&app, Some(&a.student)).await.1);
    got.sort();
    assert_eq!(
        got,
        vec![
            format!("argument_tree:{conv}"),
            format!("radar:{conv}"),
            format!("summary:{conv}"),
            format!("summary_translation:{conv}"),
        ]
    );
}
