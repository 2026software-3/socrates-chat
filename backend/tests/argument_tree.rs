// On-demand saved argument trees: synthetic data, no real model calls.
mod common;

use axum::{Router, http::StatusCode};
use common::{Script::*, *};
use serde_json::Value;
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};

const GOOD: &str = r#"{"topic_id":"TREE","title":"工具使用","nodes":[{"id":"TREE-C0","kind":"claim","label":"支持工具","source_round":1,"evidence":"我支持工具。","parent":null,"relation":null,"origin":"user"},{"id":"TREE-P1","kind":"premise","label":"節省時間","source_round":1,"evidence":"可以省時。","parent":"TREE-C0","relation":"support","origin":"user"}]}"#;

async fn fixture(pool: &PgPool, ai: Arc<FakeAi>) -> (Router, Actors, String) {
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(pool, &conv, "我支持工具。").await;
    seed_student_message(pool, &conv, "可以省時。\nuser@example.com").await;
    sqlx::query("UPDATE conversations SET status = 'ended', ended_at = now() WHERE id = $1")
        .bind(conv.parse::<uuid::Uuid>().unwrap())
        .execute(pool)
        .await
        .unwrap();
    (app, a, conv)
}

async fn request(
    app: &Router,
    method: &str,
    cookie: Option<&str>,
    conv: &str,
) -> (StatusCode, Value) {
    let path = format!("/api/conversations/{conv}/argument-tree");
    let response = send(app, json_req(method, &path, cookie, None)).await;
    let status = response.status();
    (status, json_body(response).await)
}

async fn settled(app: &Router, cookie: &str, conv: &str) -> Value {
    for _ in 0..100 {
        let (status, body) = request(app, "GET", Some(cookie), conv).await;
        assert_eq!(status, StatusCode::OK);
        if body["status"] != "pending" {
            return body;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("tree generation did not settle");
}

#[sqlx::test]
async fn owner_generates_validated_tree_and_reads_it_after_reloading(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD])]);
    let (app, a, conv) = fixture(&pool, ai.clone()).await;
    assert_eq!(
        request(&app, "GET", Some(&a.student), &conv).await.1["status"],
        "missing"
    );
    assert_eq!(
        request(&app, "POST", Some(&a.student), &conv).await.0,
        StatusCode::ACCEPTED
    );
    let tree = settled(&app, &a.student, &conv).await;
    assert_eq!(tree["status"], "ready");
    assert_eq!(tree["tree"]["node_count"], 2);
    assert_eq!(tree["tree"]["root"]["id"], "TREE-C0");
    assert!(tree["completed_at"].is_string());
    assert!(tree["prompt_version"].as_str().unwrap().contains("v2.1.1"));
    assert_eq!(
        request(&app, "POST", Some(&a.student), &conv).await.0,
        StatusCode::OK
    );
    assert_eq!(ai.request_count(), 1);
    let fresh_ai = FakeAi::with(vec![]);
    let restarted_app = test_app_with_ai(pool.clone(), fresh_ai.clone());
    assert_eq!(
        request(&restarted_app, "GET", Some(&a.student), &conv)
            .await
            .1,
        tree
    );
    assert_eq!(fresh_ai.request_count(), 0);
    let reqs = ai.requests.lock().unwrap();
    let payload: Value = serde_json::from_str(&reqs[0].messages[0].content).unwrap();
    assert_eq!(payload["rounds"].as_array().unwrap().len(), 1);
    assert!(
        payload["rounds"][0]["user_answer"]
            .as_str()
            .unwrap()
            .contains("[EMAIL]")
    );
    assert!(!reqs[0].messages[0].content.contains("user@example.com"));
    assert!(payload.get("student_name").is_none());
    assert!(payload["rounds"][0].get("source_message_indices").is_none());
}

#[sqlx::test]
async fn tree_api_enforces_login_membership_role_and_ownership(pool: PgPool) {
    let ai = FakeAi::with(vec![]);
    let (app, a, conv) = fixture(&pool, ai.clone()).await;
    import_roster(
        &app,
        &a.admin,
        &a.classroom,
        "其他學生,10000001A,other@example.com",
    )
    .await;
    let other = login_as(&app, "sub-other", "other@example.com").await;
    for method in ["GET", "POST"] {
        assert_eq!(
            request(&app, method, None, &conv).await.0,
            StatusCode::UNAUTHORIZED
        );
        for cookie in [&a.teacher, &a.admin, &a.outsider] {
            assert_eq!(
                request(&app, method, Some(cookie), &conv).await.0,
                StatusCode::FORBIDDEN
            );
        }
        assert_eq!(
            request(&app, method, Some(&other), &conv).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            request(
                &app,
                method,
                Some(&a.student),
                &uuid::Uuid::new_v4().to_string()
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(ai.request_count(), 0);
}

#[sqlx::test]
async fn tree_api_rejects_active_or_empty_dialogues_without_model_calls(pool: PgPool) {
    let ai = FakeAi::with(vec![]);
    let (app, a, conv) = fixture(&pool, ai.clone()).await;
    sqlx::query("UPDATE conversations SET status = 'active', ended_at = NULL WHERE id = $1")
        .bind(conv.parse::<uuid::Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = request(&app, "POST", Some(&a.student), &conv).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "conversation_not_ended");
    sqlx::query("UPDATE conversations SET status = 'ended', ended_at = now() WHERE id = $1")
        .bind(conv.parse::<uuid::Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM messages WHERE conversation_id = $1")
        .bind(conv.parse::<uuid::Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(&app, "POST", Some(&a.student), &conv).await.1["error"]["code"],
        "empty_conversation"
    );
    assert_eq!(ai.request_count(), 0);
}

#[sqlx::test]
async fn malformed_model_output_is_not_saved_and_failure_can_be_retried(pool: PgPool) {
    let ai = FakeAi::with(vec![
        Reply(vec![r#"{"private":"never expose this"}"#]),
        Reply(vec![GOOD]),
    ]);
    let (app, a, conv) = fixture(&pool, ai.clone()).await;
    request(&app, "POST", Some(&a.student), &conv).await;
    let failed = settled(&app, &a.student, &conv).await;
    assert_eq!(failed["status"], "failed");
    assert!(failed["tree"].is_null());
    assert!(!failed.to_string().contains("never expose"));
    let usage = json_body(send(&app, get("/api/me/usage", Some(&a.student))).await).await;
    assert_eq!(usage["used"], 0);
    assert_eq!(
        request(&app, "POST", Some(&a.student), &conv).await.0,
        StatusCode::ACCEPTED
    );
    assert_eq!(settled(&app, &a.student, &conv).await["status"], "ready");
    assert_eq!(ai.request_count(), 2);
    let usage = json_body(send(&app, get("/api/me/usage", Some(&a.student))).await).await;
    assert_eq!(usage["used"], 1);
}

#[sqlx::test]
async fn concurrent_clicks_start_only_one_generation(pool: PgPool) {
    let ai = FakeAi::with(vec![Paced(vec![GOOD], Duration::from_millis(60))]);
    let (app, a, conv) = fixture(&pool, ai.clone()).await;
    let (one, two) = tokio::join!(
        request(&app, "POST", Some(&a.student), &conv),
        request(&app, "POST", Some(&a.student), &conv),
    );
    assert_eq!(one.0, StatusCode::ACCEPTED);
    assert_eq!(two.0, StatusCode::ACCEPTED);
    assert_eq!(settled(&app, &a.student, &conv).await["status"], "ready");
    assert_eq!(ai.request_count(), 1);
}

#[sqlx::test]
async fn tree_generation_timeout_and_quota_are_safe(pool: PgPool) {
    let ai = FakeAi::with(vec![Hang]);
    let mut config = test_config();
    config.ai_total_timeout = Duration::from_millis(35);
    config.ai_usage_max = 1;
    let app = test_app_with_config(pool.clone(), ai.clone(), config);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "我支持工具。").await;
    sqlx::query("UPDATE conversations SET status = 'ended', ended_at = now() WHERE id = $1")
        .bind(conv.parse::<uuid::Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    request(&app, "POST", Some(&a.student), &conv).await;
    assert_eq!(settled(&app, &a.student, &conv).await["status"], "failed");
    assert_eq!(
        json_body(send(&app, get("/api/me/usage", Some(&a.student))).await).await["used"],
        0
    );
}

#[sqlx::test]
async fn stale_generation_can_be_retried_and_deleting_conversation_cascades(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD])]);
    let (app, a, conv) = fixture(&pool, ai).await;
    sqlx::query("INSERT INTO conversation_argument_trees (conversation_id, status, attempt, prompt_version, updated_at) VALUES ($1, 'pending', 1, 'old', now() - interval '1 day')")
        .bind(conv.parse::<uuid::Uuid>().unwrap()).execute(&pool).await.unwrap();
    assert_eq!(
        request(&app, "GET", Some(&a.student), &conv).await.1["status"],
        "failed"
    );
    request(&app, "POST", Some(&a.student), &conv).await;
    assert_eq!(settled(&app, &a.student, &conv).await["status"], "ready");
    sqlx::query("DELETE FROM conversations WHERE id = $1")
        .bind(conv.parse::<uuid::Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM conversation_argument_trees")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn saved_tree_endpoints_have_openapi_contracts() {
    let doc = serde_json::to_value(socrates_chat_backend::openapi::document()).unwrap();
    let path = &doc["paths"]["/api/conversations/{id}/argument-tree"];
    assert!(path["get"]["responses"]["200"].is_object());
    assert!(path["post"]["responses"]["202"].is_object());
}

#[sqlx::test]
async fn exhausted_quota_blocks_generation_before_any_model_call(pool: PgPool) {
    let ai = FakeAi::with(vec![]);
    let mut config = test_config();
    config.ai_usage_max = 0;
    let app = test_app_with_config(pool.clone(), ai.clone(), config);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "我支持工具。").await;
    sqlx::query("UPDATE conversations SET status = 'ended', ended_at = now() WHERE id = $1")
        .bind(conv.parse::<uuid::Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = request(&app, "POST", Some(&a.student), &conv).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["code"], "ai_usage_exceeded");
    assert_eq!(
        request(&app, "GET", Some(&a.student), &conv).await.1["status"],
        "missing"
    );
    assert_eq!(ai.request_count(), 0);
}

#[sqlx::test]
async fn a_superseded_job_cannot_overwrite_the_new_attempt(pool: PgPool) {
    let ai = FakeAi::with(vec![Paced(vec![GOOD], Duration::from_millis(80))]);
    let (app, a, conv) = fixture(&pool, ai.clone()).await;
    request(&app, "POST", Some(&a.student), &conv).await;
    sqlx::query(
        "UPDATE conversation_argument_trees SET attempt = attempt + 1 WHERE conversation_id = $1",
    )
    .bind(conv.parse::<uuid::Uuid>().unwrap())
    .execute(&pool)
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    let current = request(&app, "GET", Some(&a.student), &conv).await.1;
    assert_eq!(current["status"], "pending");
    assert!(current["tree"].is_null());
    assert_eq!(
        json_body(send(&app, get("/api/me/usage", Some(&a.student))).await).await["used"],
        0
    );
}

#[sqlx::test]
async fn archived_classrooms_allow_saved_reads_but_no_new_generation(pool: PgPool) {
    let ai = FakeAi::with(vec![Reply(vec![GOOD])]);
    let (app, a, conv) = fixture(&pool, ai.clone()).await;
    sqlx::query("UPDATE classrooms SET is_archived = true WHERE id = $1")
        .bind(a.classroom.parse::<uuid::Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = request(&app, "POST", Some(&a.student), &conv).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "classroom_archived");
    assert_eq!(ai.request_count(), 0);
    sqlx::query("UPDATE classrooms SET is_archived = false WHERE id = $1")
        .bind(a.classroom.parse::<uuid::Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    request(&app, "POST", Some(&a.student), &conv).await;
    settled(&app, &a.student, &conv).await;
    sqlx::query("UPDATE classrooms SET is_archived = true WHERE id = $1")
        .bind(a.classroom.parse::<uuid::Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(&app, "GET", Some(&a.student), &conv).await.1["status"],
        "ready"
    );
    assert_eq!(
        request(&app, "POST", Some(&a.student), &conv).await.0,
        StatusCode::OK
    );
    assert_eq!(ai.request_count(), 1);
}
