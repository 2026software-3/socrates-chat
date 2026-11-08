mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

async fn activity_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM activities")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn create_rejects_past_schedule_times(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let topic = create_topic(&app, &a, "schedule").await;
    let before = activity_count(&pool).await;
    // 開始、結束、含時區偏移後仍在過去，都要拒絕且不建立
    for sched in [
        json!({"opens_at": "2020-01-01T00:00:00Z"}),
        json!({"closes_at": "2020-01-01T00:00:00Z"}),
        json!({"opens_at": "2020-01-01T00:00:00Z", "closes_at": "2030-01-01T00:00:00Z"}),
        json!({"opens_at": "2029-01-01T00:00:00Z", "closes_at": "2020-01-01T08:00:00+08:00"}),
    ] {
        let mut body = json!({"title": "past", "topic_id": topic, "classroom_id": a.classroom});
        body.as_object_mut()
            .unwrap()
            .extend(sched.as_object().unwrap().clone());
        let res = send(
            &app,
            json_req("POST", "/api/activities", Some(&a.teacher), Some(body)),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{sched}");
        assert_eq!(json_body(res).await["error"]["code"], "schedule_in_past");
    }
    assert_eq!(activity_count(&pool).await, before);
}

#[sqlx::test]
async fn update_rejects_changed_past_times_but_keeps_unchanged_ones(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let topic = create_topic(&app, &a, "schedule").await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({
                "title": "ok", "topic_id": topic, "classroom_id": a.classroom,
                "opens_at": "2030-01-01T00:00:00Z", "closes_at": "2030-02-01T00:00:00Z"
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let id = json_body(res).await["id"].as_str().unwrap().to_string();
    let path = format!("/api/activities/{id}");
    for patch in [
        json!({"opens_at": "2020-01-01T00:00:00Z"}),
        json!({"closes_at": "2020-06-01T00:00:00Z"}),
    ] {
        let res = send(
            &app,
            json_req("PATCH", &path, Some(&a.teacher), Some(patch.clone())),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{patch}");
        assert_eq!(json_body(res).await["error"]["code"], "schedule_in_past");
    }
    let body = json_body(send(&app, get(&path, Some(&a.teacher))).await).await;
    assert_eq!(body["opens_at"], "2030-01-01T00:00:00Z");
    assert_eq!(body["closes_at"], "2030-02-01T00:00:00Z");

    // 既有的過去時間（例如已結束的活動）：沒改動就照常可更新，null 清除也可以
    sqlx::query(
        "UPDATE activities SET opens_at = now() - interval '2 days',
                closes_at = now() - interval '1 day' WHERE id = $1::uuid",
    )
    .bind(&id)
    .execute(&pool)
    .await
    .unwrap();
    for patch in [json!({"title": "renamed"}), json!({"closes_at": null})] {
        let res = send(
            &app,
            json_req("PATCH", &path, Some(&a.teacher), Some(patch.clone())),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK, "{patch}");
    }
    // 前端只到分鐘，送達時已過數秒：「現在這一分鐘」要接受
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:00Z").to_string();
    let res = send(
        &app,
        json_req(
            "PATCH",
            &path,
            Some(&a.teacher),
            Some(json!({"opens_at": now})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
}
