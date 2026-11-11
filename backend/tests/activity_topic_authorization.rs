//! F-29.7 (#392): 多題目活動的授權及資料不變性；只使用合成資料。
mod common;

use axum::{Router, http::StatusCode};
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

async fn setup(app: &Router, a: &Actors) -> (String, Vec<String>, String) {
    let topics = vec![
        create_topic(app, a, "第一題").await,
        create_topic(app, a, "第二題").await,
    ];
    let extra = create_topic(app, a, "追加題目").await;
    let res = send(
        app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({
                "title":"多題目授權", "classroom_id":a.classroom, "topic_ids":topics
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    (
        json_body(res).await["id"].as_str().unwrap().to_owned(),
        topics,
        extra,
    )
}

async fn topic_rows(pool: &PgPool, activity: &str) -> Vec<(Uuid, i32)> {
    sqlx::query_as(
        "SELECT topic_id, position FROM activity_topics WHERE activity_id=$1 ORDER BY position",
    )
    .bind(activity.parse::<Uuid>().unwrap())
    .fetch_all(pool)
    .await
    .unwrap()
}

fn without_request_id(mut body: Value) -> Value {
    body["error"].as_object_mut().unwrap().remove("request_id");
    body
}

#[sqlx::test]
async fn another_classroom_student_cannot_discover_activities_or_topics(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (activity, topics, extra) = setup(&app, &a).await;
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
    let room = create_classroom(&app, &a.teacher, "其他學生的教室").await;
    let res = import_roster(&app, &a.teacher, &room, "other-student@example.com").await;
    assert!(res.status().is_success());
    let other = login_as(&app, "other-student", "other-student@example.com").await;
    let nonexistent = Uuid::new_v4().to_string();
    let mut expected = None;
    for source in [
        json!({"activity_id":activity,"topic_id":topics[0]}),
        json!({"activity_id":activity,"topic_id":topics[1]}),
        json!({"activity_id":activity,"topic_id":extra}),
        json!({"activity_id":activity,"topic_id":nonexistent}),
        json!({"activity_id":nonexistent,"topic_id":topics[0]}),
    ] {
        let res = send(
            &app,
            json_req("POST", "/api/conversations", Some(&other), Some(source)),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body = without_request_id(json_body(res).await);
        assert_eq!(body["error"]["code"], "invalid_conversation_source");
        if let Some(ref expected) = expected {
            assert_eq!(&body, expected);
        } else {
            expected = Some(body);
        }
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM conversations WHERE activity_id=$1")
        .bind(activity.parse::<Uuid>().unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    // Same published activity and topic work for an enrolled student.
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/conversations",
            Some(&a.student),
            Some(json!({"activity_id":activity,"topic_id":topics[0]})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
}

#[sqlx::test]
async fn topic_ids_patch_requires_the_classroom_owner_and_preserves_data(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (activity, topics, extra) = setup(&app, &a).await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(&a.admin),
            Some(json!({"email":"other-teacher@example.com"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let other = login_as(&app, "other-teacher", "other-teacher@example.com").await;
    let original = topic_rows(&pool, &activity).await;
    for (cookie, status, code) in [
        (None, StatusCode::UNAUTHORIZED, "unauthorized"),
        (Some(a.student.as_str()), StatusCode::FORBIDDEN, "forbidden"),
        (
            Some(a.outsider.as_str()),
            StatusCode::FORBIDDEN,
            "forbidden",
        ),
        (Some(other.as_str()), StatusCode::NOT_FOUND, "not_found"),
    ] {
        for proposed in [
            json!([&topics[0], &topics[1], &extra]),
            json!([&topics[1], &topics[0]]),
        ] {
            let res = send(
                &app,
                json_req(
                    "PATCH",
                    &format!("/api/activities/{activity}"),
                    cookie,
                    Some(json!({"topic_ids":proposed})),
                ),
            )
            .await;
            assert_eq!(res.status(), status);
            let error = json_body(res).await;
            assert_eq!(error["error"]["code"], code);
            assert_eq!(topic_rows(&pool, &activity).await, original);
            if cookie == Some(other.as_str()) {
                let res = send(
                    &app,
                    json_req(
                        "PATCH",
                        &format!("/api/activities/{}", Uuid::new_v4()),
                        cookie,
                        Some(json!({"topic_ids":proposed})),
                    ),
                )
                .await;
                assert_eq!(res.status(), StatusCode::NOT_FOUND);
                assert_eq!(
                    without_request_id(json_body(res).await),
                    without_request_id(error)
                );
            }
        }
    }
    // Positive control: the exact append is allowed for the owner.
    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/activities/{activity}"),
            Some(&a.teacher),
            Some(json!({"topic_ids":[&topics[0],&topics[1],&extra]})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(topic_rows(&pool, &activity).await.len(), 3);
}

#[sqlx::test]
async fn archived_classroom_rejects_appending_topics_without_changing_rows(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (activity, topics, extra) = setup(&app, &a).await;
    let original = topic_rows(&pool, &activity).await;
    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/classrooms/{}", a.classroom),
            Some(&a.teacher),
            Some(json!({"is_archived":true})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    for cookie in [&a.teacher, &a.admin] {
        let res = send(
            &app,
            json_req(
                "PATCH",
                &format!("/api/activities/{activity}"),
                Some(cookie),
                Some(json!({"topic_ids":[&topics[0],&topics[1],&extra]})),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::CONFLICT);
        assert_eq!(json_body(res).await["error"]["code"], "classroom_archived");
        assert_eq!(topic_rows(&pool, &activity).await, original);
    }
}

#[sqlx::test]
async fn archived_activity_sources_hide_topic_existence_and_create_no_conversation(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (activity, topics, extra) = setup(&app, &a).await;
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
    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/classrooms/{}", a.classroom),
            Some(&a.teacher),
            Some(json!({"is_archived":true})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    // S-14 + F-02.4: unavailable sources share 400, not 409, so resource existence stays private.
    let mut expected = None;
    for (id, topic) in [
        (&activity, topics[0].clone()),
        (&activity, topics[1].clone()),
        (&activity, extra),
        (&activity, Uuid::new_v4().to_string()),
        (&Uuid::new_v4().to_string(), topics[0].clone()),
    ] {
        let res = send(
            &app,
            json_req(
                "POST",
                "/api/conversations",
                Some(&a.student),
                Some(json!({"activity_id":id,"topic_id":topic})),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body = without_request_id(json_body(res).await);
        assert_eq!(body["error"]["code"], "invalid_conversation_source");
        if let Some(ref expected) = expected {
            assert_eq!(&body, expected);
        } else {
            expected = Some(body);
        }
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM conversations WHERE activity_id=$1")
        .bind(activity.parse::<Uuid>().unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}
