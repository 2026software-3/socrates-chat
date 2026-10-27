//! 封存教室的寫入防護：檢查與寫入在同一個交易內，且不多餘的往返（#367 審查）。

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

async fn code(res: axum::response::Response) -> String {
    json_body(res).await["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

async fn archive_room(app: &axum::Router, a: &Actors) {
    let res = send(
        app,
        json_req(
            "PATCH",
            &format!("/api/classrooms/{}", a.classroom),
            Some(&a.teacher),
            Some(json!({"is_archived": true})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[sqlx::test]
async fn publishing_checks_visibility_then_classroom_then_activity_in_one_statement(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let id = published_activity_with_topic(&app, &a, "題").await;
    let post = |action: &'static str, cookie: &str, id: &str| {
        json_req(
            "POST",
            &format!("/api/activities/{id}/{action}"),
            Some(cookie),
            None,
        )
    };

    // 正常：200；他人與不存在：同樣 404
    assert_eq!(
        send(&app, post("close", &a.teacher, &id)).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        send(&app, post("publish", &a.teacher, &id)).await.status(),
        StatusCode::OK
    );
    let missing = uuid::Uuid::new_v4().to_string();
    for (cookie, target) in [
        (&a.teacher, &missing),
        (&a.outsider, &id),
        (&a.student, &id),
    ] {
        let status = send(&app, post("close", cookie, target)).await.status();
        assert!(
            status == StatusCode::NOT_FOUND || status == StatusCode::FORBIDDEN,
            "{status}"
        );
    }

    // 活動已封存：409 activity_archived，狀態不變
    send(&app, post("archive", &a.teacher, &id)).await;
    let res = send(&app, post("close", &a.teacher, &id)).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(code(res).await, "activity_archived");
    send(&app, post("unarchive", &a.teacher, &id)).await;

    // 教室已封存：409 classroom_archived，狀態不變
    archive_room(&app, &a).await;
    for action in ["publish", "close"] {
        let res = send(&app, post(action, &a.teacher, &id)).await;
        assert_eq!(res.status(), StatusCode::CONFLICT, "{action}");
        assert_eq!(code(res).await, "classroom_archived");
    }
    let status: String = sqlx::query_scalar("SELECT status FROM activities")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "published");
}

#[sqlx::test]
async fn an_archived_classroom_rejects_every_conversation_write_without_side_effects(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let activity = published_activity_with_topic(&app, &a, "題").await;
    let conv = start_from_activity(&app, &a.student, &activity).await;
    seed_student_message(&pool, &conv, "synthetic").await;
    let other = add_student(&app, &a, "sub-other", "other@example.com").await;
    archive_room(&app, &a).await;

    for (method, path, body) in [
        (
            "POST",
            format!("/api/conversations/{conv}/messages"),
            json!({"content": "no"}),
        ),
        ("POST", format!("/api/conversations/{conv}/end"), json!({})),
        (
            "POST",
            format!("/api/conversations/{conv}/summary/retry"),
            json!({}),
        ),
    ] {
        let res = send(&app, json_req(method, &path, Some(&a.student), Some(body))).await;
        assert_eq!(res.status(), StatusCode::CONFLICT, "{path}");
        assert_eq!(code(res).await, "classroom_archived");
    }
    let res = send(
        &app,
        get(
            &format!("/api/conversations/{conv}/stream"),
            Some(&a.student),
        ),
    )
    .await;
    assert!(res.status() == StatusCode::CONFLICT || res.status() == StatusCode::NOT_FOUND);
    let (turns, status): (i32, String) =
        sqlx::query_as("SELECT turn_count, status FROM conversations")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((turns, status.as_str()), (1, "active")); // 只有種子訊息那一輪
    // 別人的對話與不存在的對話仍是 404，不因封存而外露
    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/end"),
            Some(&other),
            Some(json!({})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn roster_writes_wait_for_an_in_flight_archive_and_then_refuse(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    // 另一個交易正在封存教室（尚未提交，持有該列的排他鎖）
    let mut archiving = pool.begin().await.unwrap();
    sqlx::query("UPDATE classrooms SET is_archived = true")
        .execute(&mut *archiving)
        .await
        .unwrap();
    let importing = tokio::spawn({
        let app = app.clone();
        let teacher = a.teacher.clone();
        let path = format!("/api/classrooms/{}/roster/import", a.classroom);
        async move {
            let res = send(
                &app,
                json_req(
                    "POST",
                    &path,
                    Some(&teacher),
                    Some(json!({"text": "late@example.com"})),
                ),
            )
            .await;
            (res.status(), code(res).await)
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    archiving.commit().await.unwrap();
    let (status, error) = importing.await.unwrap();
    assert_eq!(
        (status, error.as_str()),
        (StatusCode::CONFLICT, "classroom_archived")
    );
    let late: i64 =
        sqlx::query_scalar("SELECT count(*) FROM enrollments e JOIN users u ON u.id = e.user_id WHERE u.email = 'late@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(late, 0);
}
