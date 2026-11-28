//! S-20：背景 AI 工作（`ai_jobs`）。以教師重新分組為例：回 202、背景完成、可查詢、不重複、失敗退回額度、
//! 授權與中斷恢復。全部使用合成資料與假 AI。

mod common;

use std::sync::Arc;

use axum::{Router, http::StatusCode};
use common::{Script::*, *};
use serde_json::{Value, json};
use socrates_chat_backend::ai_job_store;
use sqlx::PgPool;
use uuid::Uuid;

const GROUPS: &str = r#"{"groups":[{"name":"新群","members":[1]}]}"#;

struct Scene {
    app: Router,
    a: Actors,
    activity: String,
    topic: String,
}

/// 一個活動、一場已結束且有主張的對話；AI 腳本只給重新分組用。
async fn scene(pool: &PgPool, ai: Arc<FakeAi>) -> Scene {
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let topic = seed_topic("題目").await;
    let activity = publish_activity(&app, &a, "活動", &topic).await;
    let conv = start_from_activity(&app, &a.student, &activity).await;
    sqlx::query(
        "INSERT INTO summaries (conversation_id, status, stance, reasons, claim, completed_at)
         VALUES ($1, 'ready', 's', 'r', '甲主張', now())",
    )
    .bind(conv.parse::<Uuid>().unwrap())
    .execute(pool)
    .await
    .unwrap();
    Scene {
        app,
        a,
        activity,
        topic,
    }
}

async fn regroup(s: &Scene, cookie: Option<&str>) -> (StatusCode, Value) {
    let res = send(
        &s.app,
        json_req(
            "POST",
            "/api/dashboard/class/regroup",
            cookie,
            Some(json!({"activity_id": s.activity, "topic_id": s.topic})),
        ),
    )
    .await;
    (res.status(), json_body(res).await)
}

async fn job(app: &Router, cookie: Option<&str>, id: &str) -> (StatusCode, Value) {
    let res = send(app, get(&format!("/api/ai-jobs/{id}"), cookie)).await;
    (res.status(), json_body(res).await)
}

async fn used(app: &Router, cookie: &str) -> i64 {
    let res = send(app, get("/api/me/usage", Some(cookie))).await;
    json_body(res).await["used"].as_i64().unwrap()
}

#[sqlx::test]
async fn regroup_returns_202_and_finishes_in_the_background(pool: PgPool) {
    let s = scene(&pool, FakeAi::with(vec![Reply(vec![GROUPS])])).await;
    let (status, body) = regroup(&s, Some(&s.a.teacher)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body["status"], "pending");
    let id = body["job_id"].as_str().unwrap().to_string();

    wait_for_job(&s.app, &s.a.teacher, &id).await;
    let (status, done) = job(&s.app, Some(&s.a.teacher), &id).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(done["status"], "ready");
    assert_eq!(done["kind"], "regroup");
    assert_eq!(done["label_key"], "jobs.regroup");
    assert_eq!(done["scope"]["activity_id"], s.activity);
    assert_eq!(done["scope"]["topic_id"], s.topic);
    assert!(done["error_code"].is_null());
    assert!(done["finished_at"].is_string());
    let d = json_body(send(&s.app, get("/api/dashboard/class", Some(&s.a.teacher))).await).await;
    assert_eq!(d["activities"][0]["topics"][0]["claims"][0]["name"], "新群");
    // 成功的呼叫計入額度
    assert_eq!(used(&s.app, &s.a.teacher).await, 1);
}

#[sqlx::test]
async fn a_second_request_while_running_joins_the_same_job_and_charges_once(pool: PgPool) {
    let ai = FakeAi::with(vec![Hang]);
    let s = scene(&pool, ai.clone()).await;
    let (first, a) = regroup(&s, Some(&s.a.teacher)).await;
    assert_eq!(first, StatusCode::ACCEPTED);
    wait_until_ai_called(&ai, 1).await;
    let (second, b) = regroup(&s, Some(&s.a.teacher)).await;
    assert_eq!(second, StatusCode::OK);
    assert_eq!(a["job_id"], b["job_id"]);
    assert_eq!(ai.request_count(), 1);
    assert_eq!(used(&s.app, &s.a.teacher).await, 1);
    let (_, running) = job(&s.app, Some(&s.a.teacher), a["job_id"].as_str().unwrap()).await;
    assert_eq!(running["status"], "pending", "running 對外是 pending");
}

#[sqlx::test]
async fn ai_failure_marks_the_job_failed_with_a_fixed_code_and_refunds(pool: PgPool) {
    let s = scene(&pool, FakeAi::with(vec![Fail])).await;
    let (status, body) = regroup(&s, Some(&s.a.teacher)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    wait_for_job(&s.app, &s.a.teacher, body["job_id"].as_str().unwrap()).await;
    let (_, failed) = job(&s.app, Some(&s.a.teacher), body["job_id"].as_str().unwrap()).await;
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["error_code"], "ai_unavailable");
    assert_eq!(failed["retryable"], true);
    assert_eq!(used(&s.app, &s.a.teacher).await, 0, "失敗退回額度");
}

#[sqlx::test]
async fn a_failed_job_can_be_started_again(pool: PgPool) {
    let s = scene(&pool, FakeAi::with(vec![Fail, Reply(vec![GROUPS])])).await;
    let (_, first) = regroup(&s, Some(&s.a.teacher)).await;
    wait_for_job(&s.app, &s.a.teacher, first["job_id"].as_str().unwrap()).await;
    let (status, second) = regroup(&s, Some(&s.a.teacher)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_ne!(first["job_id"], second["job_id"]);
    wait_for_job(&s.app, &s.a.teacher, second["job_id"].as_str().unwrap()).await;
    let (_, done) = job(
        &s.app,
        Some(&s.a.teacher),
        second["job_id"].as_str().unwrap(),
    )
    .await;
    assert_eq!(done["status"], "ready");
}

#[sqlx::test]
async fn only_the_owner_can_read_a_job(pool: PgPool) {
    let s = scene(&pool, FakeAi::with(vec![Reply(vec![GROUPS])])).await;
    let (_, body) = regroup(&s, Some(&s.a.teacher)).await;
    let id = body["job_id"].as_str().unwrap().to_string();
    wait_for_job(&s.app, &s.a.teacher, &id).await;
    let res = send(&s.app, get(&format!("/api/ai-jobs/{id}"), None)).await;
    assert_json_error(res, StatusCode::UNAUTHORIZED, "unauthorized").await;
    for cookie in [&s.a.student, &s.a.outsider, &s.a.admin] {
        let res = send(&s.app, get(&format!("/api/ai-jobs/{id}"), Some(cookie))).await;
        assert_json_error(res, StatusCode::NOT_FOUND, "not_found").await;
    }
    let res = send(
        &s.app,
        get(
            &format!("/api/ai-jobs/{}", Uuid::new_v4()),
            Some(&s.a.teacher),
        ),
    )
    .await;
    assert_json_error(res, StatusCode::NOT_FOUND, "not_found").await;
}

#[sqlx::test]
async fn the_active_list_includes_the_running_job_with_ids_only(pool: PgPool) {
    let ai = FakeAi::with(vec![Hang]);
    let s = scene(&pool, ai.clone()).await;
    let (_, body) = regroup(&s, Some(&s.a.teacher)).await;
    wait_until_ai_called(&ai, 1).await;
    let res = send(&s.app, get("/api/ai-jobs/active", Some(&s.a.teacher))).await;
    let list = json_body(res).await;
    let jobs = list["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0]["id"], body["job_id"]);
    assert_eq!(jobs[0]["kind"], "regroup");
    assert_eq!(jobs[0]["status"], "pending");
    assert_eq!(jobs[0]["scope"]["activity_id"], s.activity);
    // 別人看不到
    let res = send(&s.app, get("/api/ai-jobs/active", Some(&s.a.student))).await;
    let theirs = json_body(res).await;
    assert!(
        theirs["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|j| j["kind"] != "regroup"),
        "{theirs}"
    );
}

#[sqlx::test]
async fn regroup_still_checks_authorization_and_the_archive(pool: PgPool) {
    let s = scene(&pool, FakeAi::with(vec![Reply(vec![GROUPS])])).await;
    for (cookie, status) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(s.a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(s.a.outsider.as_str()), StatusCode::FORBIDDEN),
        (Some(s.a.admin.as_str()), StatusCode::FORBIDDEN),
    ] {
        assert_eq!(regroup(&s, cookie).await.0, status);
    }
    sqlx::query("UPDATE classrooms SET is_archived = true")
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = regroup(&s, Some(&s.a.teacher)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "classroom_archived");
    assert_eq!(used(&s.app, &s.a.teacher).await, 0);
}

#[sqlx::test]
async fn startup_recovery_fails_only_stale_unfinished_jobs(pool: PgPool) {
    let s = scene(&pool, FakeAi::with(vec![Hang])).await;
    let owner: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email = 'teacher@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let stale = ai_job_store::register(&pool, owner, "regroup", "k:stale", &json!({}), 900.0).await;
    let fresh = ai_job_store::register(&pool, owner, "regroup", "k:fresh", &json!({}), 900.0).await;
    let (ai_job_store::Registered::New(stale), ai_job_store::Registered::New(fresh)) =
        (stale.unwrap(), fresh.unwrap())
    else {
        panic!("both must be new");
    };
    sqlx::query("UPDATE ai_jobs SET updated_at = now() - interval '1 hour' WHERE id = $1")
        .bind(stale)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        ai_job_store::recover_orphaned(&pool, 900.0).await.unwrap(),
        1
    );
    let (_, a) = job(&s.app, Some(&s.a.teacher), &stale.to_string()).await;
    assert_eq!(a["status"], "failed");
    assert_eq!(a["error_code"], "interrupted");
    let (_, b) = job(&s.app, Some(&s.a.teacher), &fresh.to_string()).await;
    assert_eq!(b["status"], "pending");
}

#[sqlx::test]
async fn a_late_result_cannot_overwrite_an_interrupted_job(pool: PgPool) {
    let s = scene(&pool, FakeAi::with(vec![])).await;
    let owner: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email = 'teacher@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let ai_job_store::Registered::New(id) =
        ai_job_store::register(&pool, owner, "regroup", "k:late", &json!({}), 900.0)
            .await
            .unwrap()
    else {
        panic!("new");
    };
    ai_job_store::finish_failed(&pool, id, "interrupted")
        .await
        .unwrap();
    ai_job_store::finish_ready(&pool, id, &json!({"groups": 1}))
        .await
        .unwrap();
    let (_, v) = job(&s.app, Some(&s.a.teacher), &id.to_string()).await;
    assert_eq!(v["status"], "failed");
}

#[sqlx::test]
async fn a_stale_unfinished_job_does_not_block_a_new_one_for_the_same_scope(pool: PgPool) {
    let s = scene(&pool, FakeAi::with(vec![])).await;
    let owner: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email = 'teacher@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let ai_job_store::Registered::New(old) =
        ai_job_store::register(&pool, owner, "regroup", "k:same", &json!({}), 900.0)
            .await
            .unwrap()
    else {
        panic!("new");
    };
    sqlx::query("UPDATE ai_jobs SET updated_at = now() - interval '1 hour' WHERE id = $1")
        .bind(old)
        .execute(&pool)
        .await
        .unwrap();
    let again = ai_job_store::register(&pool, owner, "regroup", "k:same", &json!({}), 900.0)
        .await
        .unwrap();
    assert!(matches!(again, ai_job_store::Registered::New(id) if id != old));
    let (_, v) = job(&s.app, Some(&s.a.teacher), &old.to_string()).await;
    assert_eq!(v["error_code"], "interrupted");
}
