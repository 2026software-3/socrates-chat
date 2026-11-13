//! F-03.2 停用題目的活動選用限制（Issue #43）。
//! 涵蓋：建立／修改活動不能選停用題目（400 `invalid_topic`，不建立、不更新）、
//! 啟用題目可正常選用、不存在題目的既有行為、
//! 以及題目在活動使用後才停用時，既有活動與對話不受影響，
//! 修改活動時沿用既有（後來被停用）的題目不會讓整個更新失敗，只檢查新加入的題目。

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

async fn set_topic_active(app: &axum::Router, a: &Actors, topic: &str, active: bool) {
    let res = send(
        app,
        json_req(
            "PATCH",
            &format!("/api/topics/{topic}"),
            Some(&a.teacher),
            Some(json!({"is_active": active})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
}

async fn post_activity(
    app: &axum::Router,
    a: &Actors,
    title: &str,
    topics: Value,
) -> (StatusCode, Value) {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({"title": title, "classroom_id": a.classroom, "topic_ids": topics})),
        ),
    )
    .await;
    (res.status(), json_body(res).await)
}

async fn patch_activity(
    app: &axum::Router,
    a: &Actors,
    id: &str,
    body: Value,
) -> (StatusCode, Value) {
    let res = send(
        app,
        json_req(
            "PATCH",
            &format!("/api/activities/{id}"),
            Some(&a.teacher),
            Some(body),
        ),
    )
    .await;
    (res.status(), json_body(res).await)
}

async fn activity_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM activities")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn stored_topics(pool: &PgPool, activity: &str) -> Vec<Uuid> {
    sqlx::query_scalar(
        "SELECT topic_id FROM activity_topics WHERE activity_id = $1 ORDER BY position",
    )
    .bind(Uuid::parse_str(activity).unwrap())
    .fetch_all(pool)
    .await
    .unwrap()
}

fn uuids(ids: &[&str]) -> Vec<Uuid> {
    ids.iter().map(|s| Uuid::parse_str(s).unwrap()).collect()
}

#[sqlx::test]
async fn create_rejects_inactive_topic(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let t = seed_topic("已停用的題目").await;
    set_topic_active(&app, &a, &t, false).await;

    let (status, body) = post_activity(&app, &a, "活動", json!([t])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_topic");
    assert_eq!(activity_count(&pool).await, 0, "不得建立活動");
}

#[sqlx::test]
async fn create_rejects_when_any_topic_is_inactive(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let ok = seed_topic("啟用中").await;
    let off = seed_topic("已停用").await;
    set_topic_active(&app, &a, &off, false).await;

    let (status, body) = post_activity(&app, &a, "活動", json!([ok, off])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_topic");
    assert_eq!(activity_count(&pool).await, 0, "不得建立活動");
}

#[sqlx::test]
async fn create_rejects_inactive_topic_via_legacy_topic_id(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let t = seed_topic("已停用的題目").await;
    set_topic_active(&app, &a, &t, false).await;

    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({"title": "活動", "classroom_id": a.classroom, "topic_id": t})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(res).await["error"]["code"], "invalid_topic");
    assert_eq!(activity_count(&pool).await, 0);
}

#[sqlx::test]
async fn create_accepts_active_and_reactivated_topics(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let t1 = seed_topic("一直啟用").await;
    let t2 = seed_topic("停用後重新啟用").await;
    set_topic_active(&app, &a, &t2, false).await;
    set_topic_active(&app, &a, &t2, true).await;

    let (status, body) = post_activity(&app, &a, "活動", json!([t1, t2])).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["topic_ids"], json!([t1, t2]));
}

#[sqlx::test]
async fn create_nonexistent_topic_still_invalid_topic(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;

    let (status, body) = post_activity(&app, &a, "活動", json!([Uuid::new_v4()])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_topic");
    assert_eq!(activity_count(&pool).await, 0);
}

#[sqlx::test]
async fn inactive_topic_error_does_not_leak_internals(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let t = seed_topic("已停用的題目").await;
    set_topic_active(&app, &a, &t, false).await;

    let (status, body) = post_activity(&app, &a, "活動", json!([t])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let text = body.to_string();
    for needle in ["SELECT", "topics", "sqlx", "is_active", &t] {
        assert!(!text.contains(needle), "錯誤回應不應含 {needle}：{text}");
    }
}

#[sqlx::test]
async fn patch_rejects_switching_to_inactive_topic(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let t1 = seed_topic("原題目").await;
    let off = seed_topic("已停用").await;
    let act = create_activity(&app, &a, "活動", &t1).await;
    set_topic_active(&app, &a, &off, false).await;

    let (status, body) = patch_activity(&app, &a, &act, json!({"topic_ids": [off]})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_topic");
    assert_eq!(stored_topics(&pool, &act).await, uuids(&[&t1]), "不得更新");
}

#[sqlx::test]
async fn patch_rejects_appending_inactive_topic(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let t1 = seed_topic("原題目").await;
    let off = seed_topic("已停用").await;
    let act = create_activity(&app, &a, "活動", &t1).await;
    set_topic_active(&app, &a, &off, false).await;

    let (status, body) = patch_activity(
        &app,
        &a,
        &act,
        json!({"title": "不該被改掉", "topic_ids": [t1, off]}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_topic");
    assert_eq!(stored_topics(&pool, &act).await, uuids(&[&t1]));
    let title: String = sqlx::query_scalar("SELECT title FROM activities WHERE id = $1")
        .bind(Uuid::parse_str(&act).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(title, "活動", "400 時標題等其他欄位也不得更新");
}

#[sqlx::test]
async fn patch_rejects_inactive_topic_via_legacy_topic_id(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let t1 = seed_topic("原題目").await;
    let off = seed_topic("已停用").await;
    let act = create_activity(&app, &a, "活動", &t1).await;
    set_topic_active(&app, &a, &off, false).await;

    let (status, body) = patch_activity(&app, &a, &act, json!({"topic_id": off})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_topic");
    assert_eq!(stored_topics(&pool, &act).await, uuids(&[&t1]));
}

#[sqlx::test]
async fn patch_accepts_active_topic(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let t1 = seed_topic("原題目").await;
    let t2 = seed_topic("新題目").await;
    let act = create_activity(&app, &a, "活動", &t1).await;

    let (status, body) = patch_activity(&app, &a, &act, json!({"topic_ids": [t1, t2]})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["topic_ids"], json!([t1, t2]));
    assert_eq!(stored_topics(&pool, &act).await, uuids(&[&t1, &t2]));
}

#[sqlx::test]
async fn patch_nonexistent_topic_still_invalid_topic(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let t1 = seed_topic("原題目").await;
    let act = create_activity(&app, &a, "活動", &t1).await;

    let (status, body) =
        patch_activity(&app, &a, &act, json!({"topic_ids": [Uuid::new_v4()]})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_topic");
    assert_eq!(stored_topics(&pool, &act).await, uuids(&[&t1]));
}

#[sqlx::test]
async fn deactivating_topic_keeps_existing_activity_and_conversation(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let t = seed_topic("之後才停用").await;
    let act = publish_activity(&app, &a, "活動", &t).await;
    let conv = start_from_activity(&app, &a.student, &act).await;

    set_topic_active(&app, &a, &t, false).await;

    assert_eq!(stored_topics(&pool, &act).await, uuids(&[&t]));
    let res = send(
        &app,
        get(&format!("/api/activities/{act}"), Some(&a.teacher)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    assert_eq!(body["topic_ids"], json!([t]));
    assert_eq!(body["status"], "published");
    let conv_activity: Option<Uuid> =
        sqlx::query_scalar("SELECT activity_id FROM conversations WHERE id = $1")
            .bind(Uuid::parse_str(&conv).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(conv_activity, Some(Uuid::parse_str(&act).unwrap()));
}

#[sqlx::test]
async fn patch_keeps_existing_topic_that_was_deactivated_later(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let old = seed_topic("之後才停用").await;
    let fresh = seed_topic("新加入且啟用").await;
    let act = create_activity(&app, &a, "活動", &old).await;
    set_topic_active(&app, &a, &old, false).await;

    // 只改標題：不受停用題目影響
    let (status, _) = patch_activity(&app, &a, &act, json!({"title": "新標題"})).await;
    assert_eq!(status, StatusCode::OK);

    // 沿用已停用的既有題目，只新加入一個啟用的題目：整個更新必須成功
    let (status, body) = patch_activity(&app, &a, &act, json!({"topic_ids": [old, fresh]})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["topic_ids"], json!([old, fresh]));

    // 調換順序（皆為既有題目）也不應因為其中一個已停用而失敗
    let (status, body) = patch_activity(&app, &a, &act, json!({"topic_ids": [fresh, old]})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["topic_ids"], json!([fresh, old]));
    assert_eq!(stored_topics(&pool, &act).await, uuids(&[&fresh, &old]));
}

#[sqlx::test]
async fn patch_only_new_topics_are_checked_for_active(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let old = seed_topic("既有但已停用").await;
    let new_off = seed_topic("新加入但已停用").await;
    let act = create_activity(&app, &a, "活動", &old).await;
    set_topic_active(&app, &a, &old, false).await;
    set_topic_active(&app, &a, &new_off, false).await;

    // 既有的已停用題目沿用沒問題，但新加入的已停用題目要擋
    let (status, body) = patch_activity(&app, &a, &act, json!({"topic_ids": [old, new_off]})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_topic");
    assert_eq!(stored_topics(&pool, &act).await, uuids(&[&old]));
}
