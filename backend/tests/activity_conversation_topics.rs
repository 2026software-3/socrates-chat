//! F-29.2：學生在同一活動中依題目開始對話。

mod common;

use axum::{Router, http::StatusCode};
use common::*;
use serde_json::json;
use sqlx::PgPool;
use sqlx::migrate::Migrator;
use std::borrow::Cow;

#[sqlx::test(migrations = false)]
async fn existing_database_upgrades_without_losing_conversations(pool: PgPool) {
    let all = sqlx::migrate!();
    let legacy = Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version < 20261107000100)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    };
    legacy.run(&pool).await.unwrap();

    let app = test_app(pool.clone());
    let actors = seed_actors(&app).await;
    let (activity, first, second) = multi_topic_activity(&app, &actors).await;
    let response = start_topic(&app, Some(&actors.student), &activity, &first).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let original = json_body(response).await["id"].as_str().unwrap().to_owned();

    all.run(&pool).await.unwrap();
    all.run(&pool).await.unwrap();

    let response = send(
        &app,
        get(
            &format!("/api/conversations/{original}"),
            Some(&actors.student),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["topic_id"], first);
    let response = start_topic(&app, Some(&actors.student), &activity, &second).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(json_body(response).await["topic_id"], second);
    assert_eq!(
        start_topic(&app, Some(&actors.student), &activity, &first)
            .await
            .status(),
        StatusCode::CONFLICT
    );
}

async fn multi_topic_activity(app: &Router, actors: &Actors) -> (String, String, String) {
    let first = create_topic(app, actors, "第一題").await;
    let second = create_topic(app, actors, "第二題").await;
    let response = send(
        app,
        json_req(
            "POST",
            "/api/activities",
            Some(&actors.teacher),
            Some(json!({
                "title": "討論活動",
                "classroom_id": actors.classroom,
                "topic_ids": [&first, &second]
            })),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let activity = json_body(response).await["id"].as_str().unwrap().to_owned();
    let response = send(
        app,
        json_req(
            "POST",
            &format!("/api/activities/{activity}/publish"),
            Some(&actors.teacher),
            None,
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    (activity, first, second)
}

async fn start_topic(
    app: &Router,
    cookie: Option<&str>,
    activity: &str,
    topic: &str,
) -> axum::response::Response {
    send(
        app,
        json_req(
            "POST",
            "/api/conversations",
            cookie,
            Some(json!({"activity_id": activity, "topic_id": topic})),
        ),
    )
    .await
}

#[sqlx::test]
async fn student_can_start_the_selected_topic_in_a_multiple_topic_activity(pool: PgPool) {
    let app = test_app(pool);
    let actors = seed_actors(&app).await;
    let first = create_topic(&app, &actors, "第一題").await;
    let second = create_topic(&app, &actors, "第二題").await;

    let response = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&actors.teacher),
            Some(json!({
                "title": "討論活動",
                "classroom_id": actors.classroom,
                "topic_ids": [&first, &second]
            })),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let activity = json_body(response).await["id"].as_str().unwrap().to_owned();
    let response = send(
        &app,
        json_req(
            "POST",
            &format!("/api/activities/{activity}/publish"),
            Some(&actors.teacher),
            None,
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = send(
        &app,
        json_req(
            "POST",
            "/api/conversations",
            Some(&actors.student),
            Some(json!({"activity_id": activity, "topic_id": second})),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let conversation = json_body(response).await;
    assert_eq!(conversation["topic_id"], second);
    assert_eq!(conversation["title"], "第二題");
}

#[sqlx::test]
async fn different_topics_can_each_have_one_conversation(pool: PgPool) {
    let app = test_app(pool);
    let actors = seed_actors(&app).await;
    let first = create_topic(&app, &actors, "第一題").await;
    let second = create_topic(&app, &actors, "第二題").await;
    let response = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&actors.teacher),
            Some(json!({
                "title": "討論活動",
                "classroom_id": actors.classroom,
                "topic_ids": [&first, &second]
            })),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let activity = json_body(response).await["id"].as_str().unwrap().to_owned();
    let response = send(
        &app,
        json_req(
            "POST",
            &format!("/api/activities/{activity}/publish"),
            Some(&actors.teacher),
            None,
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    for topic in [&first, &second] {
        let response = send(
            &app,
            json_req(
                "POST",
                "/api/conversations",
                Some(&actors.student),
                Some(json!({"activity_id": activity, "topic_id": topic})),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(json_body(response).await["topic_id"], *topic);
    }
}

#[sqlx::test]
async fn same_topic_conflicts_until_deleted_without_affecting_the_other_topic(pool: PgPool) {
    let app = test_app(pool.clone());
    let actors = seed_actors(&app).await;
    let (activity, first, second) = multi_topic_activity(&app, &actors).await;

    let response = start_topic(&app, Some(&actors.student), &activity, &first).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let first_conversation = json_body(response).await["id"].as_str().unwrap().to_owned();
    let response = start_topic(&app, Some(&actors.student), &activity, &second).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let second_conversation = json_body(response).await["id"].as_str().unwrap().to_owned();
    let response = start_topic(&app, Some(&actors.student), &activity, &first).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(response).await["error"]["code"],
        "activity_already_started"
    );

    sqlx::query("UPDATE conversations SET status = 'ended', ended_at = now() WHERE id = $1")
        .bind(first_conversation.parse::<uuid::Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        start_topic(&app, Some(&actors.student), &activity, &first)
            .await
            .status(),
        StatusCode::CONFLICT
    );
    let response = send(
        &app,
        json_req(
            "DELETE",
            &format!("/api/conversations/{first_conversation}"),
            Some(&actors.student),
            None,
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = start_topic(&app, Some(&actors.student), &activity, &first).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_ne!(json_body(response).await["id"], first_conversation);
    let response = send(
        &app,
        get(
            &format!("/api/conversations/{second_conversation}"),
            Some(&actors.student),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        start_topic(&app, Some(&actors.student), &activity, &second)
            .await
            .status(),
        StatusCode::CONFLICT
    );
}

#[sqlx::test]
async fn topic_must_belong_to_activity_without_disclosing_why(pool: PgPool) {
    let app = test_app(pool);
    let actors = seed_actors(&app).await;
    let (activity, _, _) = multi_topic_activity(&app, &actors).await;
    let other = create_topic(&app, &actors, "不屬於活動的題目").await;
    let nonexistent = uuid::Uuid::new_v4().to_string();
    let response = start_topic(&app, Some(&actors.student), &activity, &other).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let mut body = json_body(response).await;
    assert_eq!(body["error"]["code"], "invalid_conversation_source");
    let response = start_topic(&app, Some(&actors.student), &activity, &nonexistent).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let mut nonexistent_body = json_body(response).await;
    body["error"].as_object_mut().unwrap().remove("request_id");
    nonexistent_body["error"]
        .as_object_mut()
        .unwrap()
        .remove("request_id");
    assert_eq!(nonexistent_body, body);
    // 個人練習不需要活動題目關聯。
    let response = send(
        &app,
        json_req(
            "POST",
            "/api/conversations",
            Some(&actors.student),
            Some(json!({"topic_id": other})),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[sqlx::test]
async fn concurrent_starts_for_one_topic_only_create_one_conversation(pool: PgPool) {
    let app = test_app(pool);
    let actors = seed_actors(&app).await;
    let (activity, _, second) = multi_topic_activity(&app, &actors).await;
    let (one, two) = tokio::join!(
        start_topic(&app, Some(&actors.student), &activity, &second),
        start_topic(&app, Some(&actors.student), &activity, &second)
    );
    let mut statuses = [one.status(), two.status()];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::CREATED, StatusCode::CONFLICT]);
}

#[sqlx::test]
async fn selecting_an_activity_topic_still_enforces_authorization_and_availability(pool: PgPool) {
    let app = test_app(pool.clone());
    let actors = seed_actors(&app).await;
    let (activity, _, second) = multi_topic_activity(&app, &actors).await;
    assert_eq!(
        start_topic(&app, None, &activity, &second).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        start_topic(&app, Some(&actors.outsider), &activity, &second)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        start_topic(&app, Some(&actors.teacher), &activity, &second)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    let other_classroom = create_classroom(&app, &actors.teacher, "另一個教室").await;
    let response =
        import_roster(&app, &actors.teacher, &other_classroom, "other@example.com").await;
    assert!(response.status().is_success());
    let other = login_as(&app, "other-student", "other@example.com").await;
    let response = start_topic(&app, Some(&other), &activity, &second).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(response).await["error"]["code"],
        "invalid_conversation_source"
    );

    sqlx::query("UPDATE activities SET opens_at = now() + interval '1 day' WHERE id = $1")
        .bind(activity.parse::<uuid::Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        start_topic(&app, Some(&actors.student), &activity, &second)
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    sqlx::query("UPDATE activities SET opens_at = NULL WHERE id = $1")
        .bind(activity.parse::<uuid::Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE classrooms SET is_archived = true WHERE id = $1")
        .bind(actors.classroom.parse::<uuid::Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        start_topic(&app, Some(&actors.student), &activity, &second)
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
}
