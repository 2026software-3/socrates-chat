//! F-28.1 教室建立與管理（S-14）。

mod common;

use axum::{Router, http::StatusCode};
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

async fn teacher_and_admin(app: &Router) -> (String, String) {
    let admin = login_as(app, "classroom-admin", "admin@example.com").await;
    let res = send(
        app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(&admin),
            Some(json!({"email": "owner@example.com"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let teacher = login_as(app, "classroom-owner", "owner@example.com").await;
    (teacher, admin)
}

async fn add_teacher(app: &Router, admin: &str, sub: &str, email: &str) -> String {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(admin),
            Some(json!({"email": email})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    login_as(app, sub, email).await
}

async fn create_classroom(app: &Router, cookie: &str, name: &str) -> Value {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/classrooms",
            Some(cookie),
            Some(json!({"name": name})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    json_body(res).await
}

#[sqlx::test]
async fn teacher_can_create_multiple_classrooms_and_edit_their_own(pool: PgPool) {
    let app = test_app(pool);
    let (teacher, _) = teacher_and_admin(&app).await;

    let first = send(
        &app,
        json_req(
            "POST",
            "/api/classrooms",
            Some(&teacher),
            Some(json!({
                "name": "  2026 春季班  ",
                "year_label": "2026",
                "description": "第一間教室"
            })),
        ),
    )
    .await;
    assert_eq!(first.status(), StatusCode::CREATED);
    let first: Value = json_body(first).await;
    assert_eq!(first["name"], "2026 春季班");
    assert_eq!(first["year_label"], "2026");
    assert_eq!(first["description"], "第一間教室");
    let first_id = first["id"].as_str().expect("classroom id");

    let second = create_classroom(&app, &teacher, "2026 秋季班").await;
    assert_ne!(first_id, second["id"].as_str().expect("classroom id"));

    let update = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/classrooms/{first_id}"),
            Some(&teacher),
            Some(json!({"name": "2026 春季研討班", "description": "更新說明"})),
        ),
    )
    .await;
    assert_eq!(update.status(), StatusCode::OK);
    let updated: Value = json_body(update).await;
    assert_eq!(updated["name"], "2026 春季研討班");
    assert_eq!(updated["description"], "更新說明");

    let listed = json_body(send(&app, get("/api/classrooms", Some(&teacher))).await).await;
    assert_eq!(listed.as_array().unwrap().len(), 2);
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == first_id)
    );
}

#[sqlx::test]
async fn classroom_name_is_globally_unique_after_trim_and_case_folding(pool: PgPool) {
    let app = test_app(pool);
    let (owner, admin) = teacher_and_admin(&app).await;
    let other_teacher = add_teacher(&app, &admin, "classroom-other", "other@example.com").await;
    let room = create_classroom(&app, &owner, "  Ethics Lab  ").await;
    let second_room = create_classroom(&app, &owner, "Second room").await;

    let admin_list = json_body(send(&app, get("/api/classrooms", Some(&admin))).await).await;
    assert_eq!(admin_list.as_array().unwrap().len(), 2);
    assert!(
        admin_list
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == room["id"])
    );

    let duplicate = send(
        &app,
        json_req(
            "POST",
            "/api/classrooms",
            Some(&other_teacher),
            Some(json!({"name": "ethics lab"})),
        ),
    )
    .await;
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(duplicate).await["error"]["code"],
        "classroom_name_taken"
    );

    let duplicate_rename = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/classrooms/{}", second_room["id"].as_str().unwrap()),
            Some(&owner),
            Some(json!({"name": " ETHICS LAB "})),
        ),
    )
    .await;
    assert_eq!(duplicate_rename.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(duplicate_rename).await["error"]["code"],
        "classroom_name_taken"
    );

    let missing = send(
        &app,
        json_req("POST", "/api/classrooms", Some(&owner), Some(json!({}))),
    )
    .await;
    assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing).await["error"]["code"],
        "invalid_classroom_name"
    );

    let blank = send(
        &app,
        json_req(
            "POST",
            "/api/classrooms",
            Some(&owner),
            Some(json!({"name": "  \t  "})),
        ),
    )
    .await;
    assert_eq!(blank.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(blank).await["error"]["code"],
        "invalid_classroom_name"
    );
}

#[sqlx::test]
async fn classroom_fields_with_invalid_or_excessive_lengths_are_rejected(pool: PgPool) {
    let app = test_app(pool);
    let (teacher, _) = teacher_and_admin(&app).await;

    for (body, expected_code) in [
        (json!({"name": "x".repeat(256)}), "invalid_classroom_name"),
        (
            json!({"name": "Room", "year_label": "y".repeat(101)}),
            "invalid_classroom_year_label",
        ),
        (
            json!({"name": "Room", "description": "d".repeat(2001)}),
            "invalid_classroom_description",
        ),
    ] {
        let response = send(
            &app,
            json_req("POST", "/api/classrooms", Some(&teacher), Some(body)),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(response).await["error"]["code"], expected_code);
    }
}

#[sqlx::test]
async fn classroom_fields_at_the_length_limits_are_accepted_and_counted_in_characters(
    pool: PgPool,
) {
    let app = test_app(pool);
    let (teacher, _) = teacher_and_admin(&app).await;

    for (name, year_label, description) in [
        ("x".repeat(255), "y".repeat(100), "d".repeat(2000)),
        ("名".repeat(255), "年".repeat(100), "說".repeat(2000)),
    ] {
        let response = send(
            &app,
            json_req(
                "POST",
                "/api/classrooms",
                Some(&teacher),
                Some(json!({
                    "name": name,
                    "year_label": year_label,
                    "description": description
                })),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let created: Value = json_body(response).await;
        assert_eq!(created["name"], name);
        assert_eq!(created["year_label"], year_label);
        assert_eq!(created["description"], description);
    }
}

#[sqlx::test]
async fn excessively_long_renamed_classroom_is_rejected(pool: PgPool) {
    let app = test_app(pool);
    let (teacher, _) = teacher_and_admin(&app).await;
    let classroom = create_classroom(&app, &teacher, "Room").await;
    let response = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/classrooms/{}", classroom["id"].as_str().unwrap()),
            Some(&teacher),
            Some(json!({"name": "x".repeat(256)})),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(response).await["error"]["code"],
        "invalid_classroom_name"
    );
}

#[sqlx::test]
async fn optional_classroom_fields_trim_blank_values_and_empty_patch_is_a_noop(pool: PgPool) {
    let app = test_app(pool);
    let (teacher, _) = teacher_and_admin(&app).await;
    let created = send(
        &app,
        json_req(
            "POST",
            "/api/classrooms",
            Some(&teacher),
            Some(json!({"name": "Room", "year_label": "  ", "description": "  "})),
        ),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let classroom: Value = json_body(created).await;
    assert!(classroom["year_label"].is_null());
    assert!(classroom["description"].is_null());

    let id = classroom["id"].as_str().unwrap();
    let unchanged = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/classrooms/{id}"),
            Some(&teacher),
            Some(json!({})),
        ),
    )
    .await;
    assert_eq!(unchanged.status(), StatusCode::OK);
    assert_eq!(
        json_body(unchanged).await["updated_at"],
        classroom["updated_at"]
    );

    let cleared = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/classrooms/{id}"),
            Some(&teacher),
            Some(json!({"year_label": "  ", "description": ""})),
        ),
    )
    .await;
    assert_eq!(cleared.status(), StatusCode::OK);
    let cleared: Value = json_body(cleared).await;
    assert!(cleared["year_label"].is_null());
    assert!(cleared["description"].is_null());
}

#[sqlx::test]
async fn other_teachers_cannot_view_or_edit_a_classroom_but_admin_can(pool: PgPool) {
    let app = test_app(pool);
    let (owner, admin) = teacher_and_admin(&app).await;
    let other_teacher = add_teacher(&app, &admin, "classroom-other", "other@example.com").await;
    let classroom = create_classroom(&app, &owner, "Owner's room").await;
    let id = classroom["id"].as_str().unwrap();

    let hidden = send(
        &app,
        get(&format!("/api/classrooms/{id}"), Some(&other_teacher)),
    )
    .await;
    assert_eq!(hidden.status(), StatusCode::NOT_FOUND);
    let hidden_body = json_body(hidden).await;
    let absent = send(
        &app,
        get(
            "/api/classrooms/00000000-0000-4000-8000-000000000000",
            Some(&other_teacher),
        ),
    )
    .await;
    assert_eq!(absent.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        json_body(absent).await["error"]["code"],
        hidden_body["error"]["code"]
    );

    let denied_update = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/classrooms/{id}"),
            Some(&other_teacher),
            Some(json!({"name": "Stolen room"})),
        ),
    )
    .await;
    assert_eq!(denied_update.status(), StatusCode::NOT_FOUND);

    let admin_update = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/classrooms/{id}"),
            Some(&admin),
            Some(json!({"description": "admin managed"})),
        ),
    )
    .await;
    assert_eq!(admin_update.status(), StatusCode::OK);
    assert_eq!(
        json_body(admin_update).await["description"],
        "admin managed"
    );
}

#[sqlx::test]
async fn only_teachers_and_admins_can_access_classroom_management(pool: PgPool) {
    let app = test_app(pool);
    let (teacher, admin) = teacher_and_admin(&app).await;
    let room = create_classroom(&app, &teacher, "Teacher room").await;
    send(
        &app,
        json_req(
            "POST",
            &format!(
                "/api/classrooms/{}/roster/import",
                room["id"].as_str().unwrap()
            ),
            Some(&admin),
            Some(json!({"text": "student@example.com"})),
        ),
    )
    .await;
    let student = login_as(&app, "classroom-student", "student@example.com").await;
    let outsider = login_as(&app, "classroom-outsider", "outsider@example.com").await;

    let classroom_id = json_body(send(&app, get("/api/classrooms", Some(&teacher))).await).await[0]
        ["id"]
        .as_str()
        .unwrap()
        .to_owned();

    for (method, path, body) in [
        (
            "POST",
            "/api/classrooms".to_owned(),
            Some(json!({"name": "Forbidden room"})),
        ),
        ("GET", "/api/classrooms".to_owned(), None),
        ("GET", format!("/api/classrooms/{classroom_id}"), None),
        (
            "PATCH",
            format!("/api/classrooms/{classroom_id}"),
            Some(json!({"name": "Forbidden room"})),
        ),
    ] {
        let anonymous = send(&app, json_req(method, &path, None, body.clone())).await;
        assert_eq!(
            anonymous.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {path}"
        );

        for cookie in [&student, &outsider] {
            let denied = send(&app, json_req(method, &path, Some(cookie), body.clone())).await;
            assert_eq!(denied.status(), StatusCode::FORBIDDEN, "{method} {path}");
            assert_eq!(json_body(denied).await["error"]["code"], "forbidden");
        }
    }
}

#[sqlx::test]
async fn empty_classroom_has_zero_student_statistics(pool: PgPool) {
    let app = test_app(pool);
    let (teacher, _) = teacher_and_admin(&app).await;

    // 建立一間沒有學生的教室
    let classroom = create_classroom(&app, &teacher, "Empty classroom").await;
    let id = classroom["id"].as_str().unwrap();

    // 查詢教室詳細資料
    let response = send(&app, get(&format!("/api/classrooms/{id}"), Some(&teacher))).await;

    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = json_body(response).await;

    // 三項統計都應該是 0
    assert_eq!(body["student_count"], 0);
    assert_eq!(body["joined_count"], 0);
    assert_eq!(body["never_logged_in_count"], 0);
}

#[sqlx::test]
async fn imported_students_are_counted_as_never_logged_in(pool: PgPool) {
    let app = test_app(pool);
    let (teacher, _) = teacher_and_admin(&app).await;

    let classroom = create_classroom(&app, &teacher, "Student statistics").await;
    let id = classroom["id"].as_str().unwrap();

    // 匯入 5 位尚未登入的學生
    let response = import_roster(
        &app,
        &teacher,
        id,
        "student1@example.com\n\
         student2@example.com\n\
         student3@example.com\n\
         student4@example.com\n\
         student5@example.com",
    )
    .await;

    assert!(response.status().is_success());

    // 查詢教室詳細資料
    let response = send(&app, get(&format!("/api/classrooms/{id}"), Some(&teacher))).await;

    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = json_body(response).await;

    assert_eq!(body["student_count"], 5);
    assert_eq!(body["joined_count"], 0);
    assert_eq!(body["never_logged_in_count"], 5);
}

#[sqlx::test]
async fn logged_in_students_are_counted_as_joined(pool: PgPool) {
    let app = test_app(pool);
    let (teacher, _) = teacher_and_admin(&app).await;

    let classroom = create_classroom(&app, &teacher, "Login statistics").await;
    let id = classroom["id"].as_str().unwrap();

    // 匯入 5 位學生
    let response = import_roster(
        &app,
        &teacher,
        id,
        "student1@example.com\n\
         student2@example.com\n\
         student3@example.com\n\
         student4@example.com\n\
         student5@example.com",
    )
    .await;

    assert!(response.status().is_success());

    // 讓其中 2 位學生登入
    login_as(&app, "student-login-1", "student1@example.com").await;
    login_as(&app, "student-login-2", "student2@example.com").await;

    // 查詢教室詳細資料
    let response = send(&app, get(&format!("/api/classrooms/{id}"), Some(&teacher))).await;

    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = json_body(response).await;

    assert_eq!(body["student_count"], 5);
    assert_eq!(body["joined_count"], 2);
    assert_eq!(body["never_logged_in_count"], 3);
}

#[sqlx::test]
async fn disabled_students_are_excluded_from_statistics(pool: PgPool) {
    let app = test_app(pool.clone());
    let (teacher, _) = teacher_and_admin(&app).await;

    let classroom = create_classroom(&app, &teacher, "Disabled student statistics").await;
    let id = classroom["id"].as_str().unwrap();

    // 匯入 5 位學生
    let response = import_roster(
        &app,
        &teacher,
        id,
        "student1@example.com\n\
         student2@example.com\n\
         student3@example.com\n\
         student4@example.com\n\
         student5@example.com",
    )
    .await;

    assert!(response.status().is_success());

    // 讓其中 2 位學生登入
    login_as(&app, "student-login-1", "student1@example.com").await;
    login_as(&app, "student-login-2", "student2@example.com").await;

    // 停用其中 1 位已登入學生
    let result = sqlx::query("UPDATE users SET disabled_at = now() WHERE email = $1")
        .bind("student1@example.com")
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(result.rows_affected(), 1);

    // 查詢教室詳細資料
    let response = send(&app, get(&format!("/api/classrooms/{id}"), Some(&teacher))).await;

    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = json_body(response).await;

    assert_eq!(body["student_count"], 4);
    assert_eq!(body["joined_count"], 1);
    assert_eq!(body["never_logged_in_count"], 3);
}

#[sqlx::test]
async fn removed_students_are_excluded_from_statistics(pool: PgPool) {
    let app = test_app(pool);
    let (teacher, _) = teacher_and_admin(&app).await;

    let classroom = create_classroom(&app, &teacher, "Removed student statistics").await;
    let id = classroom["id"].as_str().unwrap();

    // 匯入 3 位學生
    let response = import_roster(
        &app,
        &teacher,
        id,
        "student1@example.com\n\
         student2@example.com\n\
         student3@example.com",
    )
    .await;

    assert!(response.status().is_success());

    // 讓其中 2 位學生登入
    login_as(&app, "student-login-1", "student1@example.com").await;
    login_as(&app, "student-login-2", "student2@example.com").await;

    // 確認移除前的統計
    let response = send(&app, get(&format!("/api/classrooms/{id}"), Some(&teacher))).await;

    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = json_body(response).await;

    assert_eq!(body["student_count"], 3);
    assert_eq!(body["joined_count"], 2);
    assert_eq!(body["never_logged_in_count"], 1);

    // 將其中 1 位已登入學生移出教室
    let response = send(
        &app,
        json_req(
            "DELETE",
            &format!("/api/classrooms/{id}/roster/student1@example.com"),
            Some(&teacher),
            None,
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // 確認移除後的統計
    let response = send(&app, get(&format!("/api/classrooms/{id}"), Some(&teacher))).await;

    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = json_body(response).await;

    assert_eq!(body["student_count"], 2);
    assert_eq!(body["joined_count"], 1);
    assert_eq!(body["never_logged_in_count"], 1);
}

#[sqlx::test]
async fn archived_classroom_keeps_student_statistics(pool: PgPool) {
    let app = test_app(pool);
    let (teacher, _) = teacher_and_admin(&app).await;

    let classroom = create_classroom(&app, &teacher, "Archived statistics").await;
    let id = classroom["id"].as_str().unwrap();

    // 匯入 3 位學生
    let response = import_roster(
        &app,
        &teacher,
        id,
        "student1@example.com\n\
         student2@example.com\n\
         student3@example.com",
    )
    .await;

    assert!(response.status().is_success());

    // 讓其中 1 位學生登入
    login_as(&app, "student-login-1", "student1@example.com").await;

    // 封存教室
    let response = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/classrooms/{id}"),
            Some(&teacher),
            Some(json!({"is_archived": true})),
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);

    // 查詢封存後的教室詳細資料
    let response = send(&app, get(&format!("/api/classrooms/{id}"), Some(&teacher))).await;

    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = json_body(response).await;

    assert_eq!(body["is_archived"], true);
    assert_eq!(body["student_count"], 3);
    assert_eq!(body["joined_count"], 1);
    assert_eq!(body["never_logged_in_count"], 2);
}

async fn classroom_json(app: &Router, teacher: &str, id: &str) -> Value {
    let response = send(app, get(&format!("/api/classrooms/{id}"), Some(teacher))).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

#[sqlx::test]
async fn password_login_counts_a_student_as_joined(pool: PgPool) {
    let app = test_app(pool);
    let (teacher, _) = teacher_and_admin(&app).await;
    let classroom = create_classroom(&app, &teacher, "Password login statistics").await;
    let id = classroom["id"].as_str().unwrap();

    let response = send(
        &app,
        json_req(
            "POST",
            &format!("/api/classrooms/{id}/roster/import"),
            Some(&teacher),
            Some(json!({"text": "王小明,11234567A"})),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let temporary_password = json_body(response).await["credentials"][0]["temporary_password"]
        .as_str()
        .unwrap()
        .to_string();

    let body = classroom_json(&app, &teacher, id).await;
    assert_eq!(body["student_count"], 1);
    assert_eq!(body["joined_count"], 0);

    let response = send(
        &app,
        json_req(
            "POST",
            "/api/auth/login",
            None,
            Some(json!({"account": "11234567A", "password": temporary_password})),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let body = classroom_json(&app, &teacher, id).await;
    assert_eq!(body["student_count"], 1);
    assert_eq!(body["joined_count"], 1);
    assert_eq!(body["never_logged_in_count"], 0);
}

#[sqlx::test]
async fn repeated_logins_count_a_student_once(pool: PgPool) {
    let app = test_app(pool);
    let (teacher, _) = teacher_and_admin(&app).await;
    let classroom = create_classroom(&app, &teacher, "Repeated login statistics").await;
    let id = classroom["id"].as_str().unwrap();

    let response = import_roster(
        &app,
        &teacher,
        id,
        "student1@example.com\nstudent2@example.com",
    )
    .await;
    assert!(response.status().is_success());

    for _ in 0..3 {
        login_as(&app, "student-login-1", "student1@example.com").await;
    }

    let body = classroom_json(&app, &teacher, id).await;
    assert_eq!(body["student_count"], 2);
    assert_eq!(body["joined_count"], 1);
    assert_eq!(body["never_logged_in_count"], 1);
}

#[sqlx::test]
async fn create_and_update_responses_include_student_statistics(pool: PgPool) {
    let app = test_app(pool);
    let (teacher, _) = teacher_and_admin(&app).await;

    let response = send(
        &app,
        json_req(
            "POST",
            "/api/classrooms",
            Some(&teacher),
            Some(json!({"name": "Response shape"})),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = json_body(response).await;
    assert_eq!(created["student_count"], 0);
    assert_eq!(created["joined_count"], 0);
    assert_eq!(created["never_logged_in_count"], 0);
    let id = created["id"].as_str().unwrap();

    let response = import_roster(&app, &teacher, id, "student1@example.com").await;
    assert!(response.status().is_success());
    login_as(&app, "student-login-1", "student1@example.com").await;

    let response = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/classrooms/{id}"),
            Some(&teacher),
            Some(json!({"name": "Response shape renamed"})),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let updated = json_body(response).await;
    assert_eq!(updated["name"], "Response shape renamed");
    assert_eq!(updated["student_count"], 1);
    assert_eq!(updated["joined_count"], 1);
    assert_eq!(updated["never_logged_in_count"], 0);
}
