mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test]
async fn database_clock_decides_and_one_sided_schedules_work(pool: PgPool) {
    use socrates_chat_backend::activity_store;
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let id: uuid::Uuid = published_activity_with_topic(&app, &a, "schedule")
        .await
        .parse()
        .unwrap();
    // 開放與結束時間以相對於資料庫現在時間的秒數表示：(opens, closes, 是否開放中)
    for (opens, closes, open_now) in [
        (Some(-3600), Some(3600), true),
        (Some(-3600), None, true),
        (None, Some(3600), true),
        (None, None, true),
        (Some(3600), Some(7200), false),
        (Some(3600), None, false),
        (Some(-7200), Some(-3600), false),
        (None, Some(-3600), false),
    ] {
        sqlx::query(
            "UPDATE activities
             SET opens_at = now() + make_interval(secs => $2::float8),
                 closes_at = now() + make_interval(secs => $3::float8)
             WHERE id = $1",
        )
        .bind(id)
        .bind(opens)
        .bind(closes)
        .execute(&pool)
        .await
        .unwrap();
        let listed = activity_store::list_published_for_student(&pool, "student@example.com")
            .await
            .unwrap();
        assert_eq!(
            listed.iter().any(|a| a.id == id),
            open_now,
            "{opens:?} {closes:?}"
        );
        let mut conn = pool.acquire().await.unwrap();
        let allowed = activity_store::find_startable(&mut conn, id, "student@example.com")
            .await
            .unwrap();
        assert_eq!(allowed.is_some(), open_now, "{opens:?} {closes:?}");
    }
    let err = sqlx::query("UPDATE activities SET opens_at=now(), closes_at=now() WHERE id=$1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap_err();
    assert_eq!(
        err.as_database_error().unwrap().constraint(),
        Some("activities_schedule_check")
    );
    for status in ["draft", "closed"] {
        sqlx::query("UPDATE activities SET status=$2, opens_at=NULL, closes_at=NULL WHERE id=$1")
            .bind(id)
            .bind(status)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            activity_store::list_published_for_student(&pool, "student@example.com")
                .await
                .unwrap()
                .is_empty()
        );
        let mut conn = pool.acquire().await.unwrap();
        assert!(
            activity_store::find_startable(&mut conn, id, "student@example.com")
                .await
                .unwrap()
                .is_none()
        );
    }
}

#[sqlx::test]
async fn schedule_authorization_and_existing_conversations(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let id = published_activity_with_topic(&app, &a, "authorization").await;
    let conv = start_from_activity(&app, &a.student, &id).await;
    let path = format!("/api/activities/{id}");
    for (cookie, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            send(
                &app,
                json_req("PATCH", &path, cookie, Some(json!({"closes_at": null})))
            )
            .await
            .status(),
            expected
        );
    }
    send(
        &app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(&a.admin),
            Some(json!({"email":"other@example.com"})),
        ),
    )
    .await;
    let other = login_as(&app, "other", "other@example.com").await;
    assert_eq!(
        send(
            &app,
            json_req(
                "PATCH",
                &path,
                Some(&other),
                Some(json!({"closes_at": null}))
            )
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    // API 不接受新設定的過去時間，直接寫資料庫模擬活動已結束
    sqlx::query("UPDATE activities SET closes_at = '2000-01-01T00:00:00Z' WHERE id = $1::uuid")
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        send(
            &app,
            get(&format!("/api/conversations/{conv}"), Some(&a.student))
        )
        .await
        .status(),
        StatusCode::OK
    );
    // The fake AI fails, but accepting and saving the student's message still succeeds.
    assert_eq!(
        student_says(&app, &a, &conv, "synthetic reason").await.0,
        StatusCode::OK
    );
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM messages WHERE conversation_id=$1 AND role='student'",
    )
    .bind(conv.parse::<uuid::Uuid>().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    sqlx::query("INSERT INTO summaries (conversation_id, status, stance, reasons) VALUES ($1, 'ready', 'synthetic stance', 'synthetic reasons')")
        .bind(conv.parse::<uuid::Uuid>().unwrap()).execute(&pool).await.unwrap();
    assert_eq!(
        send(
            &app,
            get(
                &format!("/api/conversations/{conv}/summary"),
                Some(&a.student)
            )
        )
        .await
        .status(),
        StatusCode::OK
    );
}

#[sqlx::test]
async fn schedule_round_trip_validation_and_null_patch(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let topic = create_topic(&app, &a, "schedule").await;
    for end in ["2030-01-01T00:00:00Z", "2029-12-31T23:59:59Z"] {
        let res = send(
            &app,
            json_req(
                "POST",
                "/api/activities",
                Some(&a.teacher),
                Some(json!({
                    "title":"invalid", "topic_id":topic, "classroom_id":a.classroom,
                    "opens_at":"2030-01-01T00:00:00Z", "closes_at":end
                })),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(res).await["error"]["code"], "invalid_schedule");
    }
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({
                "title": "scheduled", "topic_id": topic, "classroom_id": a.classroom,
                "opens_at": "2030-01-01T08:00:00+08:00", "closes_at": "2030-01-02T00:00:00Z"
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let body = json_body(res).await;
    assert_eq!(body["opens_at"], "2030-01-01T00:00:00Z");
    let path = format!("/api/activities/{}", body["id"].as_str().unwrap());
    let res = send(
        &app,
        json_req(
            "PATCH",
            &path,
            Some(&a.teacher),
            Some(json!({"closes_at": "2030-01-01T00:00:00Z"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(res).await["error"]["code"], "invalid_schedule");
    let body = json_body(send(&app, get(&path, Some(&a.teacher))).await).await;
    assert_eq!(body["closes_at"], "2030-01-02T00:00:00Z");
    let res = send(
        &app,
        json_req(
            "PATCH",
            &path,
            Some(&a.teacher),
            Some(json!({"opens_at": null})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    assert!(body["opens_at"].is_null());
    assert_eq!(body["closes_at"], "2030-01-02T00:00:00Z");
}

#[sqlx::test]
async fn unavailable_schedule_hides_catalog_and_rejects_direct_start(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let id = published_activity_with_topic(&app, &a, "schedule").await;
    for (opens, closes) in [
        (Some("2100-01-01T00:00:00Z"), None),
        // 已結束：API 不接受新設定的過去時間，直接寫資料庫模擬
        (None, Some("2000-01-01T00:00:00Z")),
    ] {
        sqlx::query("UPDATE activities SET opens_at = $2::timestamptz, closes_at = $3::timestamptz WHERE id = $1::uuid")
            .bind(&id)
            .bind(opens)
            .bind(closes)
            .execute(&pool)
            .await
            .unwrap();
        let catalog = json_body(send(&app, get("/api/catalog", Some(&a.student))).await).await;
        assert!(catalog["activities"].as_array().unwrap().is_empty());
        let res = send(
            &app,
            json_req(
                "POST",
                "/api/conversations",
                Some(&a.student),
                Some(json!({"activity_id": id})),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            json_body(res).await["error"]["code"],
            "invalid_conversation_source"
        );
    }
}

#[sqlx::test]
async fn archived_classroom_rejects_schedule_changes_and_can_be_restored(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let id = published_activity_with_topic(&app, &a, "schedule").await;
    let path = format!("/api/classrooms/{}", a.classroom);
    let res = send(
        &app,
        json_req(
            "PATCH",
            &path,
            Some(&a.teacher),
            Some(json!({"is_archived": true})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_body(res).await["is_archived"], true);
    let activity_path = format!("/api/activities/{id}");
    let res = send(
        &app,
        json_req(
            "PATCH",
            &activity_path,
            Some(&a.teacher),
            Some(json!({"opens_at": null})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(res).await["error"]["code"], "classroom_archived");
    assert_eq!(
        send(&app, get(&activity_path, Some(&a.teacher)))
            .await
            .status(),
        StatusCode::OK
    );
    let res = send(
        &app,
        json_req(
            "PATCH",
            &path,
            Some(&a.teacher),
            Some(json!({"is_archived": false})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_body(res).await["is_archived"], false);
}

#[sqlx::test]
async fn archived_rooms_are_read_only_but_personal_practice_remains_available(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let topic = create_topic(&app, &a, "archiving").await;
    let id = publish_activity(&app, &a, "archiving", &topic).await;
    let conv = start_from_activity(&app, &a.student, &id).await;
    let room_path = format!("/api/classrooms/{}", a.classroom);
    // Ordinary students and outsiders cannot archive classrooms.
    for (cookie, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            send(
                &app,
                json_req(
                    "PATCH",
                    &room_path,
                    cookie,
                    Some(json!({"is_archived":true}))
                )
            )
            .await
            .status(),
            expected
        );
    }
    send(
        &app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(&a.admin),
            Some(json!({"email":"other@example.com"})),
        ),
    )
    .await;
    let other = login_as(&app, "other", "other@example.com").await;
    assert_eq!(
        send(
            &app,
            json_req(
                "PATCH",
                &room_path,
                Some(&other),
                Some(json!({"is_archived":true}))
            )
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            &app,
            json_req(
                "PATCH",
                &room_path,
                Some(&a.admin),
                Some(json!({"is_archived":true}))
            )
        )
        .await
        .status(),
        StatusCode::OK
    );
    for (method, path, cookie, body) in [
        (
            "POST",
            "/api/activities".to_string(),
            &a.teacher,
            json!({"title":"no", "classroom_id":a.classroom,"topic_id":topic}),
        ),
        (
            "POST",
            format!("/api/activities/{id}/publish"),
            &a.teacher,
            json!({}),
        ),
        (
            "POST",
            format!("/api/activities/{id}/close"),
            &a.admin,
            json!({}),
        ),
        ("PATCH", room_path.clone(), &a.teacher, json!({"name":"no"})),
        (
            "POST",
            format!("{room_path}/roster/import"),
            &a.teacher,
            json!({"text":"synthetic@example.com"}),
        ),
        (
            "POST",
            format!("/api/conversations/{conv}/messages"),
            &a.student,
            json!({"content":"no"}),
        ),
        (
            "POST",
            format!("/api/conversations/{conv}/end"),
            &a.student,
            json!({}),
        ),
        (
            "POST",
            format!("/api/conversations/{conv}/summary/retry"),
            &a.student,
            json!({}),
        ),
    ] {
        let res = send(&app, json_req(method, &path, Some(cookie), Some(body))).await;
        assert_eq!(res.status(), StatusCode::CONFLICT, "{method} {path}");
        assert_eq!(json_body(res).await["error"]["code"], "classroom_archived");
    }
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/conversations",
            Some(&a.student),
            Some(json!({"activity_id":id})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(res).await["error"]["code"],
        "invalid_conversation_source"
    );
    assert_eq!(
        send(
            &app,
            get(&format!("/api/conversations/{conv}"), Some(&a.student))
        )
        .await
        .status(),
        StatusCode::OK
    );
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/conversations",
            Some(&a.student),
            Some(json!({"topic_id":topic})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    // Restore preserves the roster and activity, so the existing conversation is writable again.
    assert_eq!(
        send(
            &app,
            json_req(
                "PATCH",
                &room_path,
                Some(&a.admin),
                Some(json!({"is_archived":false}))
            )
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        student_says(&app, &a, &conv, "synthetic reason").await.0,
        StatusCode::OK
    );
}

#[test]
fn openapi_documents_schedule_and_archiving() {
    let doc = serde_json::to_value(socrates_chat_backend::openapi::document()).unwrap();
    for schema in ["NewActivity", "ActivityPatch", "Activity"] {
        for field in ["opens_at", "closes_at"] {
            assert!(doc["components"]["schemas"][schema]["properties"][field].is_object());
        }
    }
    for schema in ["Classroom", "ClassroomPatch"] {
        assert!(doc["components"]["schemas"][schema]["properties"]["is_archived"].is_object());
    }
    for (path, method) in [
        ("/api/activities", "post"),
        ("/api/activities/{id}", "patch"),
    ] {
        assert!(
            doc["paths"][path][method]["responses"]["400"]["description"]
                .as_str()
                .unwrap()
                .contains("invalid_schedule")
        );
    }
}
