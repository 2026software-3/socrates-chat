//! S-01.4：匯入每一行都是姓名、學號、選填電子郵件。

use socrates_chat_backend::roster::parse_student_roster;

mod common;
use common::*;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test]
async fn migrations_can_be_applied_again(pool: PgPool) {
    sqlx::migrate!().run(&pool).await.unwrap();
    sqlx::migrate!().run(&pool).await.unwrap();
}

#[sqlx::test]
async fn database_enforces_student_identifier_and_name_invariants(pool: PgPool) {
    for query in [
        "INSERT INTO users (student_id, imported_name) VALUES ('12345678a', '合成姓名')",
        "INSERT INTO users (student_id) VALUES ('12345678A')",
        "INSERT INTO users (student_id, imported_name, is_admin) VALUES ('12345678A', '合成姓名', true)",
        "INSERT INTO users (student_id, imported_name) VALUES ('12345678A', repeat('字', 31))",
    ] {
        assert!(sqlx::query(query).execute(&pool).await.is_err());
    }
    sqlx::query("INSERT INTO users (student_id, imported_name) VALUES ('12345678A', '合成姓名')")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        sqlx::query("INSERT INTO users (student_id, imported_name) VALUES ('12345678A', '另一人')")
            .execute(&pool)
            .await
            .is_err()
    );
    let owner: uuid::Uuid =
        sqlx::query_scalar("INSERT INTO users (email) VALUES ('teacher@example.com') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    let room: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO classrooms (name, created_by) VALUES ('測試教室', $1) RETURNING id",
    )
    .bind(owner)
    .fetch_one(&pool)
    .await
    .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    assert!(
        socrates_chat_backend::roster_store::add_user(&mut conn, room, owner)
            .await
            .is_err()
    );
}

#[test]
fn parses_students_and_reports_invalid_lines_without_echoing_personal_data() {
    let parsed = parse_student_roster(
        " 王小明 ,11234567a, WANG@example.com \n李小華,12345678b,\n姓名,學號,電子郵件\n,12345678C,x@example.com\n王小明,wrong,x@example.com\n",
    );
    assert_eq!(parsed.students.len(), 2);
    assert_eq!(parsed.students[0].name, "王小明");
    assert_eq!(parsed.students[0].student_id, "11234567A");
    assert_eq!(
        parsed.students[0].email.as_deref(),
        Some("wang@example.com")
    );
    assert_eq!(parsed.students[1].email, None);
    assert_eq!(
        parsed
            .invalid
            .iter()
            .map(|row| row.line)
            .collect::<Vec<_>>(),
        vec![3, 4, 5]
    );
    let errors = serde_json::to_string(&parsed.invalid).unwrap();
    assert!(!errors.contains("王小明"));
    assert!(!errors.contains("12345678"));
}

#[test]
fn supports_quoted_csv_fields_and_rejects_malformed_quotes() {
    let parsed = parse_student_roster(
        "\"王,小明\",12345678A,one@example.com\n\"李\"\"小華\",12345679B,\n\"bad,12345670C,\n",
    );
    assert_eq!(parsed.students.len(), 2);
    assert_eq!(parsed.students[0].name, "王,小明");
    assert_eq!(parsed.students[1].name, "李\"小華");
    assert_eq!(parsed.invalid.len(), 1);
}

#[test]
fn imported_name_limit_is_thirty_unicode_characters_after_trimming() {
    let input = format!(
        " {} ,12345678A,\n{},12345679A,",
        "字".repeat(30),
        "字".repeat(31)
    );
    let parsed = parse_student_roster(&input);
    assert_eq!(parsed.students.len(), 1);
    assert_eq!(parsed.students[0].name, "字".repeat(30));
    assert_eq!(parsed.invalid[0].line, 2);
}

#[test]
fn rejects_nul_in_imported_name_before_database_write() {
    let parsed = parse_student_roster("小\0明,12345678A,");
    assert!(parsed.students.is_empty());
    assert_eq!(parsed.invalid[0].reason, "invalid_format");
}

#[sqlx::test]
async fn teacher_import_creates_one_account_and_credential_for_a_student_without_email(
    pool: PgPool,
) {
    let app = test_app(pool.clone());
    let actors = seed_actors(&app).await;
    let request = || {
        json_req(
            "POST",
            &format!("/api/classrooms/{}/roster/import", actors.classroom),
            Some(&actors.teacher),
            Some(json!({"text": "王小明,11234567a,"})),
        )
    };
    let response = send(&app, request()).await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let result = json_body(response).await;
    assert_eq!(
        result
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        ["added", "credentials", "errors", "existing", "invalid"]
    );
    assert_eq!(result["added"], 1);
    assert_eq!(result["credentials"][0]["student_id"], "11234567A");
    assert!(result["credentials"][0]["temporary_password"].is_string());
    assert_eq!(result["errors"], 0);
    let result = json_body(send(&app, request()).await).await;
    assert_eq!(result["existing"], 1);
    assert_eq!(result["credentials"], json!([]));
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM users WHERE student_id = '11234567A' AND email IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test]
async fn existing_student_can_join_another_class_but_conflicts_are_rejected(pool: PgPool) {
    let app = test_app(pool.clone());
    let actors = seed_actors(&app).await;
    let first = json_req(
        "POST",
        &format!("/api/classrooms/{}/roster/import", actors.classroom),
        Some(&actors.teacher),
        Some(json!({"text":"甲,12345678a,student2@example.com"})),
    );
    let original = json_body(send(&app, first).await).await;
    assert_eq!(original["credentials"].as_array().unwrap().len(), 1);
    let room = create_classroom(&app, &actors.teacher, "另一個教室").await;
    let request = |text: &str| {
        json_req(
            "POST",
            &format!("/api/classrooms/{room}/roster/import"),
            Some(&actors.teacher),
            Some(json!({"text": text})),
        )
    };
    let joined = json_body(send(&app, request("改名,12345678A,student2@example.com")).await).await;
    assert_eq!(joined["added"], 1);
    assert_eq!(joined["credentials"], json!([]));
    let conflict = json_body(
        send(
            &app,
            request("甲,12345678A,other@example.com\n乙,87654321B,student2@example.com"),
        )
        .await,
    )
    .await;
    assert_eq!(conflict["added"], 0);
    assert_eq!(conflict["invalid"].as_array().unwrap().len(), 2);
    assert_eq!(conflict["errors"], 2);
    assert_eq!(conflict["invalid"][0]["reason"], "student_id_conflict");
    assert_eq!(conflict["invalid"][1]["reason"], "email_conflict");
    assert!(!conflict.to_string().contains("12345678"));
    assert!(!conflict.to_string().contains("甲"));
    let row: (String, String, i64) = sqlx::query_as("SELECT u.display_name, u.email, count(e.user_id) FROM users u JOIN enrollments e ON e.user_id = u.id WHERE u.student_id = '12345678A' GROUP BY u.id").fetch_one(&pool).await.unwrap();
    assert_eq!(row, ("甲".into(), "student2@example.com".into(), 2));
}

#[sqlx::test]
async fn another_teacher_importing_existing_student_never_receives_a_password(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let response = send(
        &app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(&a.admin),
            Some(json!({"email":"second@example.com"})),
        ),
    )
    .await;
    assert_eq!(response.status(), axum::http::StatusCode::NO_CONTENT);
    let teacher = login_as(&app, "second-teacher", "second@example.com").await;
    let classroom = create_classroom(&app, &teacher, "另一位教師的教室").await;
    let response = send(
        &app,
        json_req(
            "POST",
            &format!("/api/classrooms/{classroom}/roster/import"),
            Some(&teacher),
            Some(json!({"text":"不同姓名,10000000a,student@example.com"})),
        ),
    )
    .await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let result = json_body(response).await;
    assert_eq!(result["added"], 1);
    assert_eq!(result["credentials"], json!([]));
}

#[sqlx::test]
async fn import_checks_identity_classroom_ownership_and_archive_status(pool: PgPool) {
    use axum::http::StatusCode;
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let path = format!("/api/classrooms/{}/roster/import", a.classroom);
    let call = |cookie: Option<&str>| {
        json_req(
            "POST",
            &path,
            cookie,
            Some(json!({"text": "合成姓名,54321098a,"})),
        )
    };
    assert_eq!(
        send(&app, call(None)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&app, call(Some(&a.student))).await.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&app, call(Some(&a.outsider))).await.status(),
        StatusCode::FORBIDDEN
    );
    send(
        &app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(&a.admin),
            Some(json!({"email":"other-teacher@example.com"})),
        ),
    )
    .await;
    let other = login_as(&app, "other-teacher", "other-teacher@example.com").await;
    assert_eq!(
        send(&app, call(Some(&other))).await.status(),
        StatusCode::NOT_FOUND
    );
    let archived = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/classrooms/{}", a.classroom),
            Some(&a.teacher),
            Some(json!({"is_archived": true})),
        ),
    )
    .await;
    assert_eq!(archived.status(), StatusCode::OK);
    assert_eq!(
        send(&app, call(Some(&a.teacher))).await.status(),
        StatusCode::CONFLICT
    );
}

/// 管理者新增第二位教師並登入，回傳該教師的 cookie。
async fn second_teacher(app: &axum::Router, admin: &str) -> String {
    let response = send(
        app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(admin),
            Some(json!({"email":"second@example.com"})),
        ),
    )
    .await;
    assert_eq!(response.status(), axum::http::StatusCode::NO_CONTENT);
    login_as(app, "second-teacher", "second@example.com").await
}

async fn import_text(
    app: &axum::Router,
    cookie: &str,
    classroom: &str,
    text: &str,
) -> serde_json::Value {
    let response = send(
        app,
        json_req(
            "POST",
            &format!("/api/classrooms/{classroom}/roster/import"),
            Some(cookie),
            Some(json!({"text": text})),
        ),
    )
    .await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    json_body(response).await
}

async fn email_of(pool: &PgPool, student_id: &str) -> Option<String> {
    sqlx::query_scalar("SELECT email FROM users WHERE student_id = $1")
        .bind(student_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// 學生的電子郵件只由學生自己填：匯入不會替既有帳號補上，不論誰匯入。
#[sqlx::test]
async fn import_never_attaches_an_email_to_an_existing_student(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    import_text(&app, &a.teacher, &a.classroom, "甲,12345678A,").await;
    let other = second_teacher(&app, &a.admin).await;
    let room = create_classroom(&app, &other, "另一位教師的教室").await;

    // 先不帶信箱把學生加進自己的教室，再補信箱：兩步都不能改動帳號。
    let enrolled = import_text(&app, &other, &room, "甲,12345678A,").await;
    assert_eq!(enrolled["added"], 1);
    assert_eq!(email_of(&pool, "12345678A").await, None);
    for (cookie, classroom) in [
        (&other, &room),
        (&a.teacher, &a.classroom),
        (&a.admin, &room),
    ] {
        let result =
            import_text(&app, cookie, classroom, "甲,12345678A,attacker@example.com").await;
        assert_eq!(result["errors"], 0);
        assert!(result.get("email_updated").is_none());
        assert_eq!(email_of(&pool, "12345678A").await, None);
    }
}

#[sqlx::test]
async fn teacher_can_enroll_an_existing_student_in_a_new_classroom_even_with_an_email_on_the_line(
    pool: PgPool,
) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    import_text(&app, &a.teacher, &a.classroom, "甲,12345678A,").await;
    let other = second_teacher(&app, &a.admin).await;
    let room = create_classroom(&app, &other, "另一位教師的教室").await;

    let result = import_text(&app, &other, &room, "甲,12345678A,attacker@example.com").await;

    assert_eq!(result["added"], 1);
    assert_eq!(result["credentials"], json!([]));
    assert_eq!(email_of(&pool, "12345678A").await, None);
}
