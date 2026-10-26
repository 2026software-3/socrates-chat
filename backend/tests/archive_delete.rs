//! S-14：教室、活動、對話的封存（可復原）與永久刪除（二次確認）。

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

async fn status_of(
    app: &axum::Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    body: serde_json::Value,
) -> StatusCode {
    send(app, json_req(method, path, cookie, Some(body)))
        .await
        .status()
}

async fn count(pool: &PgPool, table: &'static str) -> i64 {
    let sql: &'static str = match table {
        "classrooms" => "SELECT count(*) FROM classrooms",
        "enrollments" => "SELECT count(*) FROM enrollments",
        "activities" => "SELECT count(*) FROM activities",
        _ => "SELECT count(*) FROM conversations",
    };
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

async fn deleted_count(pool: &PgPool, action: &'static str) -> Option<i32> {
    sqlx::query_scalar("SELECT item_count FROM audit_logs WHERE action = $1")
        .bind(action)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn audit_actions(pool: &PgPool) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT action, target_email FROM audit_logs WHERE action LIKE 'classroom_%' OR action LIKE 'activity_%' ORDER BY created_at, id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn deleting_a_classroom_needs_the_name_and_removes_everything_inside(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let activity = published_activity_with_topic(&app, &a, "題").await;
    let conv = start_from_activity(&app, &a.student, &activity).await;
    let path = format!("/api/classrooms/{}", a.classroom);

    // 未登入、學生、名單外帳號都不能刪除
    assert_eq!(
        status_of(
            &app,
            "DELETE",
            &path,
            None,
            json!({"confirm_name": "測試教室"})
        )
        .await,
        StatusCode::UNAUTHORIZED
    );
    for cookie in [&a.student, &a.outsider] {
        assert_eq!(
            status_of(
                &app,
                "DELETE",
                &path,
                Some(cookie),
                json!({"confirm_name": "測試教室"})
            )
            .await,
            StatusCode::FORBIDDEN
        );
    }
    // 名稱不符
    assert_eq!(
        status_of(
            &app,
            "DELETE",
            &path,
            Some(&a.teacher),
            json!({"confirm_name": "別的"})
        )
        .await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(count(&pool, "classrooms").await, 1);

    assert_eq!(
        status_of(
            &app,
            "DELETE",
            &path,
            Some(&a.teacher),
            json!({"confirm_name": " 測試教室 "})
        )
        .await,
        StatusCode::NO_CONTENT
    );
    for table in ["classrooms", "enrollments", "activities", "conversations"] {
        assert_eq!(count(&pool, table).await, 0, "{table}");
    }
    // 名單一併刪除，學生不在任何教室名單內，回「尚未開通」
    assert_eq!(
        send(
            &app,
            get(&format!("/api/conversations/{conv}"), Some(&a.student))
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        audit_actions(&pool).await,
        [("classroom_deleted".to_string(), "測試教室".to_string())]
    );
    assert_eq!(deleted_count(&pool, "classroom_deleted").await, Some(1));
    // 名稱釋出
    create_classroom(&app, &a.teacher, "測試教室").await;
}

#[sqlx::test]
async fn an_archived_classroom_can_be_deleted_and_archiving_is_audited(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let path = format!("/api/classrooms/{}", a.classroom);
    for archived in [true, false, true] {
        assert_eq!(
            status_of(
                &app,
                "PATCH",
                &path,
                Some(&a.teacher),
                json!({"is_archived": archived, "reason": "學期結束"})
            )
            .await,
            StatusCode::OK
        );
    }
    assert_eq!(
        status_of(
            &app,
            "DELETE",
            &path,
            Some(&a.admin),
            json!({"confirm_name": "測試教室"})
        )
        .await,
        StatusCode::NO_CONTENT
    );
    let reason: Option<String> = sqlx::query_scalar(
        "SELECT reason FROM audit_logs WHERE action = 'classroom_archived' ORDER BY created_at LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reason.as_deref(), Some("學期結束"));
    // 空教室刪除時對話數量為 0
    assert_eq!(deleted_count(&pool, "classroom_deleted").await, Some(0));
    let actions: Vec<String> = audit_actions(&pool)
        .await
        .into_iter()
        .map(|x| x.0)
        .collect();
    assert_eq!(
        actions,
        [
            "classroom_archived",
            "classroom_restored",
            "classroom_archived",
            "classroom_deleted"
        ]
    );
}

#[sqlx::test]
async fn archiving_an_activity_hides_it_from_students_and_can_be_undone(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let id = published_activity_with_topic(&app, &a, "題").await;
    let archive = format!("/api/activities/{id}/archive");
    let unarchive = format!("/api/activities/{id}/unarchive");

    assert_eq!(
        status_of(&app, "POST", &archive, Some(&a.student), json!({})).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status_of(&app, "POST", &archive, Some(&a.teacher), json!({})).await,
        StatusCode::OK
    );
    // 教師預設清單隱藏、已封存檢視看得到
    let list = json_body(send(&app, get("/api/activities", Some(&a.teacher))).await).await;
    assert!(list["items"].as_array().unwrap().is_empty());
    let list =
        json_body(send(&app, get("/api/activities?archived=true", Some(&a.teacher))).await).await;
    assert_eq!(list["items"][0]["id"], id.as_str());
    assert_eq!(list["items"][0]["is_archived"], true);
    // 學生看不到、不能開新對話
    let catalog = json_body(send(&app, get("/api/catalog", Some(&a.student))).await).await;
    assert!(catalog["activities"].as_array().unwrap().is_empty());
    assert_eq!(
        status_of(
            &app,
            "POST",
            "/api/conversations",
            Some(&a.student),
            json!({"activity_id": id})
        )
        .await,
        StatusCode::BAD_REQUEST
    );

    assert_eq!(
        status_of(&app, "POST", &unarchive, Some(&a.teacher), json!({})).await,
        StatusCode::OK
    );
    start_from_activity(&app, &a.student, &id).await;
    let actions: Vec<String> = audit_actions(&pool)
        .await
        .into_iter()
        .map(|x| x.0)
        .collect();
    assert_eq!(actions, ["activity_archived", "activity_restored"]);
}

#[sqlx::test]
async fn deleting_an_activity_removes_its_conversations_even_when_archived(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let id = published_activity_with_topic(&app, &a, "題").await;
    let conv = start_from_activity(&app, &a.student, &id).await;
    let path = format!("/api/activities/{id}");

    assert_eq!(
        status_of(
            &app,
            "DELETE",
            &path,
            Some(&a.teacher),
            json!({"confirm_title": "x"})
        )
        .await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(count(&pool, "conversations").await, 1);
    // 封存的教室也能刪除其中的活動
    send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/classrooms/{}", a.classroom),
            Some(&a.teacher),
            Some(json!({"is_archived": true})),
        ),
    )
    .await;
    assert_eq!(
        status_of(
            &app,
            "DELETE",
            &path,
            Some(&a.teacher),
            json!({"confirm_title": "題"})
        )
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(count(&pool, "activities").await, 0);
    assert_eq!(count(&pool, "conversations").await, 0);
    assert_eq!(
        send(
            &app,
            get(&format!("/api/conversations/{conv}"), Some(&a.student))
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        status_of(
            &app,
            "DELETE",
            &path,
            Some(&a.teacher),
            json!({"confirm_title": "題"})
        )
        .await,
        StatusCode::NOT_FOUND
    );
    assert!(
        audit_actions(&pool)
            .await
            .iter()
            .any(|x| x.0 == "activity_deleted" && x.1 == "題")
    );
    assert_eq!(deleted_count(&pool, "activity_deleted").await, Some(1));
}

#[sqlx::test]
async fn a_student_archives_only_their_own_view_of_a_conversation(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let id = published_activity_with_topic(&app, &a, "題").await;
    let conv = start_from_activity(&app, &a.student, &id).await;
    let archive = format!("/api/conversations/{conv}/archive");
    let unarchive = format!("/api/conversations/{conv}/unarchive");

    // 他人的對話與未登入
    assert_eq!(
        status_of(&app, "POST", &archive, None, json!({})).await,
        StatusCode::UNAUTHORIZED
    );
    let other = add_student(&app, &a, "sub-other", "other@example.com").await;
    assert_eq!(
        status_of(&app, "POST", &archive, Some(&other), json!({})).await,
        StatusCode::NOT_FOUND
    );

    assert_eq!(
        status_of(&app, "POST", &archive, Some(&a.student), json!({})).await,
        StatusCode::NO_CONTENT
    );
    let list = json_body(send(&app, get("/api/conversations", Some(&a.student))).await).await;
    assert!(list["items"].as_array().unwrap().is_empty());
    let list = json_body(
        send(
            &app,
            get("/api/conversations?archived=true", Some(&a.student)),
        )
        .await,
    )
    .await;
    assert_eq!(list["items"][0]["id"], conv.as_str());
    assert_eq!(list["items"][0]["is_archived"], true);
    // 仍算該活動已有一場
    assert_eq!(
        status_of(
            &app,
            "POST",
            "/api/conversations",
            Some(&a.student),
            json!({"activity_id": id})
        )
        .await,
        StatusCode::CONFLICT
    );

    // 教室封存後仍可復原與刪除自己的對話
    send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/classrooms/{}", a.classroom),
            Some(&a.teacher),
            Some(json!({"is_archived": true})),
        ),
    )
    .await;
    assert_eq!(
        status_of(&app, "POST", &unarchive, Some(&a.student), json!({})).await,
        StatusCode::NO_CONTENT
    );
    let list = json_body(send(&app, get("/api/conversations", Some(&a.student))).await).await;
    assert_eq!(list["items"][0]["is_archived"], false);
    assert_eq!(
        status_of(
            &app,
            "DELETE",
            &format!("/api/conversations/{conv}"),
            Some(&a.student),
            json!({})
        )
        .await,
        StatusCode::NO_CONTENT
    );
}

#[sqlx::test]
async fn the_deleted_conversation_count_ignores_personal_practice(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let other = add_student(&app, &a, "sub-other", "other@example.com").await;
    let topic = create_topic(&app, &a, "題").await;
    let first = publish_activity(&app, &a, "活動一", &topic).await;
    let second = publish_activity(&app, &a, "活動二", &topic).await;
    // 教室內三場對話：兩位學生在活動一、其中一位另在活動二
    start_from_activity(&app, &a.student, &first).await;
    start_from_activity(&app, &a.student, &second).await;
    start_from_activity(&app, &other, &first).await;
    // 兩場個人練習（沒有教室）
    for cookie in [&a.student, &other] {
        assert_eq!(
            status_of(
                &app,
                "POST",
                "/api/conversations",
                Some(cookie),
                json!({"topic_id": topic})
            )
            .await,
            StatusCode::CREATED
        );
    }
    assert_eq!(count(&pool, "conversations").await, 5);

    assert_eq!(
        status_of(
            &app,
            "DELETE",
            &format!("/api/classrooms/{}", a.classroom),
            Some(&a.teacher),
            json!({"confirm_name": "測試教室"})
        )
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(deleted_count(&pool, "classroom_deleted").await, Some(3));
    // 個人練習不受影響
    assert_eq!(count(&pool, "conversations").await, 2);
}
