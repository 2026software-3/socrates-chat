mod common;

use axum::http::{StatusCode, header};
use chrono::{DateTime, Utc};
use common::*;
use serde_json::json;
use socrates_chat_backend::student_notifications::record_student_number_correction;
use sqlx::PgPool;
use uuid::Uuid;

const PATH: &str = "/api/notifications";

#[tokio::test]
async fn an_anonymous_request_cannot_list_or_dismiss_notifications() {
    let app = test_app(PgPool::connect_lazy("postgres://unused@127.0.0.1:1/unused").unwrap());
    assert_json_error(
        send(&app, get(PATH, None)).await,
        StatusCode::UNAUTHORIZED,
        "unauthorized",
    )
    .await;
    let path = format!("{PATH}/{}/read", Uuid::new_v4());
    let request = json_req("POST", &path, None, None);
    assert_json_error(
        send(&app, request).await,
        StatusCode::UNAUTHORIZED,
        "unauthorized",
    )
    .await;
}

#[sqlx::test]
async fn only_the_recipient_can_read_and_dismiss_an_unread_notification(pool: PgPool) {
    sqlx::migrate!().run(&pool).await.unwrap();
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let student_id: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email = 'student@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let created_at: DateTime<Utc> = "2020-01-01T12:34:56Z".parse().unwrap();
    let mut tx = pool.begin().await.unwrap();
    record_student_number_correction(&mut tx, student_id)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(
        json_body(send(&app, get(PATH, Some(&a.student))).await).await,
        json!([])
    );

    let mut tx = pool.begin().await.unwrap();
    record_student_number_correction(&mut tx, student_id)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let id: Uuid = sqlx::query_scalar(
        "UPDATE student_notifications SET created_at = $2 WHERE user_id = $1 RETURNING id",
    )
    .bind(student_id)
    .bind(created_at)
    .fetch_one(&pool)
    .await
    .unwrap();
    let imported = import_roster(
        &app,
        &a.admin,
        &a.classroom,
        "另一個學生,10000001B,other@example.com",
    )
    .await;
    assert!(imported.status().is_success());
    let other = login_as(&app, "sub-other-student", "other@example.com").await;

    let path = format!("{PATH}/{id}/read");
    assert_eq!(
        json_body(send(&app, get(PATH, Some(&other))).await).await,
        json!([])
    );
    assert_json_error(
        send(&app, json_req("POST", &path, Some(&other), None)).await,
        StatusCode::NOT_FOUND,
        "not_found",
    )
    .await;
    // 教師也會收到站內通知（F-40.9），但看不到學生的；只有管理者身分與名單外帳號不能用
    assert_eq!(
        json_body(send(&app, get(PATH, Some(&a.teacher))).await).await,
        json!([])
    );
    for cookie in [&a.admin, &a.outsider] {
        assert_eq!(
            send(&app, get(PATH, Some(cookie))).await.status(),
            StatusCode::FORBIDDEN
        );
    }

    // An old notification persists across repeated reads until the student dismisses it.
    for _ in 0..2 {
        let response = send(&app, get(PATH, Some(&a.student))).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            json_body(response).await,
            json!([{
                "id": id,
                "kind": "student_number_corrected",
                "created_at": created_at,
            }])
        );
    }

    let mut read_times = Vec::new();
    for _ in 0..2 {
        let response = send(&app, json_req("POST", &path, Some(&a.student), None)).await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        read_times.push(
            sqlx::query_scalar::<_, DateTime<Utc>>(
                "SELECT read_at FROM student_notifications WHERE id = $1",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        );
    }
    assert_eq!(read_times[0], read_times[1]);
    let next_session = login_as(&app, "sub-student", "student@example.com").await;
    assert_eq!(
        json_body(send(&app, get(PATH, Some(&next_session))).await).await,
        json!([])
    );
}
