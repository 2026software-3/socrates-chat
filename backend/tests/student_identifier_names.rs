//! API 的學生識別欄位命名（#446）：`student_number` 一律是學號，學生的使用者 UUID 另用
//! `student_user_id`（總結列表、對話原文）或 `id`（學生列表），`student_id` 不再出現在任何對外契約。
//! 只使用合成資料。

mod common;

use axum::{Router, http::StatusCode};
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// `seed_actors` 匯入的預設學生（`student@example.com`）的合成學號。
const STUDENT_NUMBER: &str = "10000000A";

async fn ok_json(app: &Router, path: &str, cookie: &str) -> Value {
    let res = send(app, get(path, Some(cookie))).await;
    assert_eq!(res.status(), StatusCode::OK, "{path}");
    json_body(res).await
}

fn items(body: &Value) -> &Vec<Value> {
    body["items"].as_array().unwrap()
}

/// 直接寫入一份已完成的總結並結束對話，再補上一則訊息，讓原文端點有內容。
async fn finish(pool: &PgPool, conversation: &str) {
    let id: Uuid = conversation.parse().unwrap();
    sqlx::query(
        "INSERT INTO summaries (conversation_id, status, stance, reasons, claim)
         VALUES ($1, 'ready', '立場', '理由', '主張')",
    )
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("UPDATE conversations SET status = 'ended', ended_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    seed_student_message(pool, conversation, "合成的學生訊息").await;
}

/// 教室名單上這位預設學生的使用者 UUID 與學號（取自學生列表）。
async fn student_row(app: &Router, a: &Actors) -> (String, Value) {
    let students = ok_json(
        app,
        &format!("/api/classrooms/{}/students", a.classroom),
        &a.teacher,
    )
    .await;
    let row = items(&students)
        .iter()
        .find(|r| r["email"] == "student@example.com")
        .expect("the seeded student is on the roster")
        .clone();
    (row["id"].as_str().unwrap().to_string(), row)
}

#[sqlx::test]
async fn student_list_names_the_number_student_number_and_the_uuid_id(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;

    for cookie in [&a.teacher, &a.admin] {
        let students = ok_json(
            &app,
            &format!("/api/classrooms/{}/students", a.classroom),
            cookie,
        )
        .await;
        let row = items(&students)
            .iter()
            .find(|r| r["email"] == "student@example.com")
            .unwrap();
        assert_eq!(row["student_number"], STUDENT_NUMBER);
        assert!(Uuid::parse_str(row["id"].as_str().unwrap()).is_ok());
        assert!(row.get("student_id").is_none(), "{row}");
    }
}

#[sqlx::test]
async fn import_credentials_name_the_number_student_number(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;

    let res = import_roster(&app, &a.teacher, &a.classroom, "合成學生,22222222b").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    let credential = &body["credentials"][0];
    assert_eq!(credential["student_number"], "22222222B");
    assert!(credential["temporary_password"].is_string());
    assert!(credential.get("student_id").is_none(), "{credential}");
}

#[sqlx::test]
async fn summary_list_and_transcript_name_the_user_uuid_student_user_id(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    finish(&pool, &conv).await;
    let (uuid, _) = student_row(&app, &a).await;

    for cookie in [&a.teacher, &a.admin] {
        let list = ok_json(&app, "/api/summaries", cookie).await;
        let row = &items(&list)[0];
        assert_eq!(row["student_user_id"], uuid);
        assert!(row.get("student_id").is_none(), "{row}");
        assert!(
            !row.to_string().contains(STUDENT_NUMBER),
            "the UUID field must not carry the student number"
        );

        let transcript = ok_json(&app, &format!("/api/summaries/{conv}/messages"), cookie).await;
        assert_eq!(transcript["student_user_id"], uuid);
        assert!(transcript.get("student_id").is_none(), "{transcript}");
    }
}

#[sqlx::test]
async fn student_filter_still_takes_the_user_uuid_from_the_summary_row(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    finish(&pool, &conv).await;

    let list = ok_json(&app, "/api/summaries", &a.teacher).await;
    let uuid = items(&list)[0]["student_user_id"].as_str().unwrap();
    let filtered = ok_json(&app, &format!("/api/summaries?student={uuid}"), &a.teacher).await;
    assert_eq!(items(&filtered).len(), 1);
    assert_eq!(items(&filtered)[0]["conversation_id"], conv);

    // 學號不是 UUID，不能當 `student` 篩選條件
    let res = send(
        &app,
        get(
            &format!("/api/summaries?student={STUDENT_NUMBER}"),
            Some(&a.teacher),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_json_error(res, StatusCode::BAD_REQUEST, "invalid_filter").await;
}

#[sqlx::test]
async fn delete_confirmation_uses_confirm_student_number_and_ignores_the_old_name(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (uuid, _) = student_row(&app, &a).await;
    let path = format!("/api/students/{uuid}");

    // 舊欄位名稱不再被接受：視為沒有確認
    let res = send(
        &app,
        json_req(
            "DELETE",
            &path,
            Some(&a.teacher),
            Some(json!({"confirm_student_id": STUDENT_NUMBER})),
        ),
    )
    .await;
    assert_json_error(res, StatusCode::BAD_REQUEST, "confirmation_mismatch").await;
    // 學生 UUID 不是學號，不能拿來確認
    let res = send(
        &app,
        json_req(
            "DELETE",
            &path,
            Some(&a.teacher),
            Some(json!({"confirm_student_number": uuid})),
        ),
    )
    .await;
    assert_json_error(res, StatusCode::BAD_REQUEST, "confirmation_mismatch").await;

    let res = send(
        &app,
        json_req(
            "DELETE",
            &path,
            Some(&a.teacher),
            Some(json!({"confirm_student_number": STUDENT_NUMBER.to_lowercase()})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
}

#[sqlx::test]
async fn access_rules_hold_for_the_renamed_fields(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    finish(&pool, &conv).await;
    let (uuid, _) = student_row(&app, &a).await;

    // 其他教師：看不到這間教室的名單、總結與對話原文，回應也不含學號或 UUID
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(&a.admin),
            Some(json!({"email": "t2@example.com"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let other_teacher = login_as(&app, "sub-t2", "t2@example.com").await;
    let students = format!("/api/classrooms/{}/students", a.classroom);
    let transcript = format!("/api/summaries/{conv}/messages");
    for path in [&students, &transcript] {
        let res = send(&app, get(path, Some(&other_teacher))).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{path}");
        let body = text_body(res).await;
        assert!(
            !body.contains(STUDENT_NUMBER) && !body.contains(&uuid),
            "{path}"
        );
    }
    let list = ok_json(&app, "/api/summaries", &other_teacher).await;
    assert!(items(&list).is_empty());

    // 學生、名單外帳號、未登入
    for (cookie, status) in [
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        (None, StatusCode::UNAUTHORIZED),
    ] {
        for path in [&students, &transcript, &"/api/summaries".to_string()] {
            let res = send(&app, get(path, cookie)).await;
            assert_eq!(res.status(), status, "{path}");
            let body = text_body(res).await;
            assert!(
                !body.contains(STUDENT_NUMBER) && !body.contains(&uuid),
                "{path}"
            );
        }
        let res = send(
            &app,
            json_req(
                "DELETE",
                &format!("/api/students/{uuid}"),
                cookie,
                Some(json!({"confirm_student_number": STUDENT_NUMBER})),
            ),
        )
        .await;
        assert_eq!(res.status(), status);
    }
}
