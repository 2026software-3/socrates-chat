//! F-30.5：更正學號、權限、交易與不含個資的稽核（合成資料）。
mod common;

use axum::{Router, http::StatusCode};
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

fn path(id: Uuid) -> String {
    format!("/api/students/{id}/student-number")
}

async fn correct(
    app: &Router,
    id: Uuid,
    cookie: Option<&str>,
    body: Value,
) -> axum::response::Response {
    send(app, json_req("PUT", &path(id), cookie, Some(body))).await
}

async fn student_id(pool: &PgPool) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE student_id = '10000000A'")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn assert_number(pool: &PgPool, id: Uuid, expected: &str) {
    let number: String = sqlx::query_scalar("SELECT student_id FROM users WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(number, expected);
}

async fn assert_no_effects(pool: &PgPool) {
    let count: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM audit_logs WHERE action = 'student_number_corrected') + (SELECT count(*) FROM student_notifications)")
        .fetch_one(pool).await.unwrap();
    assert_eq!(count, 0);
}

// 錯誤本文的契約必須精確相同，不能多帶學號、電子郵件或資料庫錯誤內容。
async fn assert_json_error(response: axum::response::Response, status: StatusCode, code: &str) {
    assert_eq!(response.status(), status);
    let request_id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_string();
    assert!(!request_id.is_empty());
    assert_eq!(
        json_body(response).await,
        json!({"error": {"code": code, "request_id": request_id}})
    );
}

#[tokio::test]
async fn anonymous_requests_are_rejected_and_openapi_defines_the_contract() {
    let app = test_app(PgPool::connect_lazy("postgres://unused@127.0.0.1:1/unused").unwrap());
    assert_json_error(
        correct(
            &app,
            Uuid::new_v4(),
            None,
            json!({"student_number":"12345678a"}),
        )
        .await,
        StatusCode::UNAUTHORIZED,
        "unauthorized",
    )
    .await;
    let spec = json_body(send(&app, get("/api/openapi.json", None)).await).await;
    let op = &spec["paths"]["/api/students/{id}/student-number"]["put"];
    assert!(op["responses"]["204"].is_object());
    assert!(op["responses"]["409"].is_object());
    assert!(op["requestBody"].is_object());
}

#[sqlx::test]
async fn teacher_corrects_a_shared_student_and_records_only_ids_and_time(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let id = student_id(&pool).await;
    let second = create_classroom(&app, &a.teacher, "第二間教室").await;
    assert!(
        import_roster(&app, &a.teacher, &second, "學生,10000000A")
            .await
            .status()
            .is_success()
    );
    let response = correct(
        &app,
        id,
        Some(&a.teacher),
        json!({"student_number":"12345678b"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(
        http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes()
            .is_empty()
    );
    assert_number(&pool, id, "12345678B").await;
    let numbers: Vec<String> =
        sqlx::query_scalar("SELECT student_id FROM enrollments WHERE user_id = $1")
            .bind(id)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(numbers, vec!["12345678B", "12345678B"]);
    let audit: Value = sqlx::query_scalar(
        "SELECT to_jsonb(a) FROM audit_logs a WHERE action = 'student_number_corrected'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let actor: Uuid = sqlx::query_scalar("SELECT created_by FROM classrooms WHERE id = $1")
        .bind(Uuid::parse_str(&a.classroom).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(audit["actor_id"], json!(actor));
    assert_eq!(audit["target_email"], json!(id.to_string()));
    assert!(audit["created_at"].is_string());
    for field in ["old_value", "new_value", "reason"] {
        assert!(audit[field].is_null());
    }
    let text = audit.to_string();
    for secret in ["10000000A", "12345678B", "12345678b", "student@example.com"] {
        assert!(!text.contains(secret));
    }
    let notices = json_body(send(&app, get("/api/notifications", Some(&a.student))).await).await;
    assert_eq!(notices.as_array().unwrap().len(), 1);
    assert_eq!(notices[0]["kind"], "student_number_corrected");
    assert_eq!(notices[0].as_object().unwrap().len(), 3);
    assert!(!notices.to_string().contains("12345678"));
    // 重送相同的學號不是更正，不重複寫入稽核與通知。
    assert_eq!(
        correct(
            &app,
            id,
            Some(&a.teacher),
            json!({"student_number":"12345678b"})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM student_notifications WHERE user_id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test]
async fn admin_can_correct_an_unenrolled_student_without_email(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let id = student_id(&pool).await;
    sqlx::query("UPDATE users SET email = NULL WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM enrollments WHERE user_id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        correct(
            &app,
            id,
            Some(&a.admin),
            json!({"student_number":"87654321z"})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_number(&pool, id, "87654321Z").await;
}

#[sqlx::test]
async fn invalid_duplicate_and_email_changes_are_rejected_without_side_effects(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let id = student_id(&pool).await;
    assert!(
        import_roster(&app, &a.teacher, &a.classroom, "另一位學生,12345678B")
            .await
            .status()
            .is_success()
    );
    for body in [
        json!({"student_number":"bad"}),
        json!({"student_number":"１２３４５６７８A"}),
        json!({"student_number":"12345678"}),
        json!({"student_number":"12345678AB"}),
        json!({"student_number":""}),
    ] {
        assert_json_error(
            correct(&app, id, Some(&a.teacher), body).await,
            StatusCode::BAD_REQUEST,
            "invalid_student_number",
        )
        .await;
    }
    assert_json_error(
        correct(
            &app,
            id,
            Some(&a.admin),
            json!({"student_number":"12345678b"}),
        )
        .await,
        StatusCode::CONFLICT,
        "student_number_in_use",
    )
    .await;
    for body in [
        json!({}),
        json!({"student_number":null}),
        json!({"student_number":123}),
        json!({"student_number":"87654321A", "email":"new@example.com"}),
    ] {
        assert_json_error(
            correct(&app, id, Some(&a.admin), body).await,
            StatusCode::BAD_REQUEST,
            "invalid_body",
        )
        .await;
    }
    assert_number(&pool, id, "10000000A").await;
    assert_no_effects(&pool).await;
}

#[sqlx::test]
async fn unauthorized_callers_cannot_probe_accounts_or_validate_numbers(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let id = student_id(&pool).await;
    for cookie in [&a.student, &a.outsider] {
        for target in [id, Uuid::new_v4()] {
            assert_json_error(
                correct(
                    &app,
                    target,
                    Some(cookie),
                    json!({"student_number":"12345678B"}),
                )
                .await,
                StatusCode::FORBIDDEN,
                "forbidden",
            )
            .await;
        }
    }
    // 已移出教師全部教室的學生，和不存在的學生，回相同 404；先授權再驗證輸入。
    sqlx::query("DELETE FROM enrollments WHERE user_id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    for target in [id, Uuid::new_v4()] {
        for body in [
            json!({"student_number":"12345678B"}),
            json!({"student_number":"bad"}),
            json!({}),
        ] {
            assert_json_error(
                correct(&app, target, Some(&a.teacher), body).await,
                StatusCode::NOT_FOUND,
                "not_found",
            )
            .await;
        }
    }
    assert_no_effects(&pool).await;
}

#[sqlx::test]
async fn a_teacher_can_correct_students_shared_with_another_teacher(pool: PgPool) {
    let app = test_app(pool.clone());
    seed_actors(&app).await;
    sqlx::query("INSERT INTO teachers (email) VALUES ('second@example.com')")
        .execute(&pool)
        .await
        .unwrap();
    let second = login_as(&app, "second-teacher", "second@example.com").await;
    let room = create_classroom(&app, &second, "他人的教室").await;
    let id = student_id(&pool).await;
    assert_json_error(
        correct(
            &app,
            id,
            Some(&second),
            json!({"student_number":"12345678B"}),
        )
        .await,
        StatusCode::NOT_FOUND,
        "not_found",
    )
    .await;
    assert!(
        import_roster(&app, &second, &room, "學生,10000000A")
            .await
            .status()
            .is_success()
    );
    assert_eq!(
        correct(
            &app,
            id,
            Some(&second),
            json!({"student_number":"12345678B"})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_number(&pool, id, "12345678B").await;
}

#[sqlx::test]
async fn a_failed_notification_rolls_back_the_number_and_audit(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let id = student_id(&pool).await;
    sqlx::query("ALTER TABLE student_notifications ADD CONSTRAINT fail_correction CHECK (kind <> 'student_number_corrected')").execute(&pool).await.unwrap();
    assert_json_error(
        correct(
            &app,
            id,
            Some(&a.teacher),
            json!({"student_number":"12345678B"}),
        )
        .await,
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal",
    )
    .await;
    assert_number(&pool, id, "10000000A").await;
    assert_no_effects(&pool).await;
}

#[sqlx::test]
async fn concurrent_corrections_cannot_assign_the_same_number_to_two_students(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let first = student_id(&pool).await;
    assert!(
        import_roster(&app, &a.teacher, &a.classroom, "另一位學生,12345678B")
            .await
            .status()
            .is_success()
    );
    let second: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE student_id = '12345678B'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let (one, two) = tokio::join!(
        correct(
            &app,
            first,
            Some(&a.teacher),
            json!({"student_number":"87654321Z"})
        ),
        correct(
            &app,
            second,
            Some(&a.admin),
            json!({"student_number":"87654321z"})
        ),
    );
    let statuses = [one.status(), two.status()];
    assert!(statuses.contains(&StatusCode::NO_CONTENT));
    assert!(statuses.contains(&StatusCode::CONFLICT));
    let notices: i64 = sqlx::query_scalar("SELECT count(*) FROM student_notifications")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(notices, 1);
}

#[sqlx::test]
async fn staff_accounts_cannot_be_corrected_and_missing_admin_targets_are_hidden(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let teacher: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email = 'teacher@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_json_error(
        correct(
            &app,
            teacher,
            Some(&a.admin),
            json!({"student_number":"12345678A"}),
        )
        .await,
        StatusCode::CONFLICT,
        "not_a_student",
    )
    .await;
    assert_json_error(
        correct(
            &app,
            Uuid::new_v4(),
            Some(&a.admin),
            json!({"student_number":"12345678A"}),
        )
        .await,
        StatusCode::NOT_FOUND,
        "not_found",
    )
    .await;
    assert_no_effects(&pool).await;
}

#[sqlx::test(migrations = false)]
async fn audit_migration_preserves_existing_records_and_can_be_run_twice(pool: PgPool) {
    use sqlx::migrate::Migrator;
    let all = sqlx::migrate!();
    Migrator::with_migrations(
        all.iter()
            .filter(|m| m.version < 20261124000000)
            .cloned()
            .collect(),
    )
    .run(&pool)
    .await
    .unwrap();
    let id: Uuid = sqlx::query_scalar("INSERT INTO audit_logs (action, target_email) VALUES ('roster_added', 'synthetic-user-id') RETURNING id")
        .fetch_one(&pool).await.unwrap();
    all.run(&pool).await.unwrap();
    all.run(&pool).await.unwrap();
    let target: String = sqlx::query_scalar("SELECT target_email FROM audit_logs WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(target, "synthetic-user-id");
    sqlx::query("INSERT INTO audit_logs (action, target_email) VALUES ('student_number_corrected', 'another-synthetic-user-id')")
        .execute(&pool).await.unwrap();
}
