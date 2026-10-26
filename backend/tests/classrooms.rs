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
