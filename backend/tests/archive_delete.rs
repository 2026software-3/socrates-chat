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

#[sqlx::test]
async fn delete_without_a_valid_body_is_a_400_after_the_404_check(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let activity = published_activity_with_topic(&app, &a, "題").await;
    let missing = uuid::Uuid::new_v4();
    for (path, absent) in [
        (
            format!("/api/activities/{activity}"),
            format!("/api/activities/{missing}"),
        ),
        (
            format!("/api/classrooms/{}", a.classroom),
            format!("/api/classrooms/{missing}"),
        ),
    ] {
        // 沒有 body、body 不是 JSON、缺少確認欄位：一律是專案格式的 400
        for req in [
            axum::http::Request::builder()
                .method("DELETE")
                .uri(&path)
                .header(axum::http::header::ORIGIN, APP_URL)
                .header(axum::http::header::COOKIE, &a.teacher)
                .body(axum::body::Body::empty())
                .unwrap(),
            axum::http::Request::builder()
                .method("DELETE")
                .uri(&path)
                .header(axum::http::header::ORIGIN, APP_URL)
                .header(axum::http::header::COOKIE, &a.teacher)
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from("not json"))
                .unwrap(),
            json_req("DELETE", &path, Some(&a.teacher), Some(json!({}))),
        ] {
            let res = send(&app, req).await;
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{path}");
            assert_eq!(
                json_body(res).await["error"]["code"],
                "confirmation_mismatch"
            );
        }
        // 不存在的對象仍然是 404，不受 body 影響
        let res = send(
            &app,
            axum::http::Request::builder()
                .method("DELETE")
                .uri(&absent)
                .header(axum::http::header::ORIGIN, APP_URL)
                .header(axum::http::header::COOKIE, &a.teacher)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{absent}");
    }
    assert_eq!(count(&pool, "classrooms").await, 1);
    assert_eq!(count(&pool, "activities").await, 1);
}

#[sqlx::test]
async fn archived_filter_is_strict(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    for (path, cookie) in [
        ("/api/activities", &a.teacher),
        ("/api/conversations", &a.student),
    ] {
        for ok in ["true", "false"] {
            let res = send(&app, get(&format!("{path}?archived={ok}"), Some(cookie))).await;
            assert_eq!(res.status(), StatusCode::OK, "{path} {ok}");
        }
        for bad in ["1", "True", "yes", ""] {
            let res = send(&app, get(&format!("{path}?archived={bad}"), Some(cookie))).await;
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{path} {bad:?}");
            assert_eq!(json_body(res).await["error"]["code"], "invalid_archived");
        }
    }
}

#[sqlx::test]
async fn an_archived_activity_cannot_be_published_or_closed_until_restored(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let id = published_activity_with_topic(&app, &a, "題").await;
    status_of(
        &app,
        "POST",
        &format!("/api/activities/{id}/archive"),
        Some(&a.teacher),
        json!({}),
    )
    .await;
    for action in ["publish", "close"] {
        let res = send(
            &app,
            json_req(
                "POST",
                &format!("/api/activities/{id}/{action}"),
                Some(&a.teacher),
                None,
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::CONFLICT, "{action}");
        assert_eq!(json_body(res).await["error"]["code"], "activity_archived");
    }
    status_of(
        &app,
        "POST",
        &format!("/api/activities/{id}/unarchive"),
        Some(&a.teacher),
        json!({}),
    )
    .await;
    assert_eq!(
        status_of(
            &app,
            "POST",
            &format!("/api/activities/{id}/close"),
            Some(&a.teacher),
            json!({})
        )
        .await,
        StatusCode::OK
    );
}

#[sqlx::test]
async fn archive_audit_is_written_once_per_real_transition(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let id = published_activity_with_topic(&app, &a, "題").await;
    let archive = format!("/api/activities/{id}/archive");
    for _ in 0..2 {
        assert_eq!(
            status_of(&app, "POST", &archive, Some(&a.teacher), json!({})).await,
            StatusCode::OK
        );
    }
    let classroom = format!("/api/classrooms/{}", a.classroom);
    for _ in 0..2 {
        status_of(
            &app,
            "PATCH",
            &classroom,
            Some(&a.teacher),
            json!({"is_archived": true}),
        )
        .await;
    }
    let actions: Vec<String> = audit_actions(&pool)
        .await
        .into_iter()
        .map(|x| x.0)
        .collect();
    assert_eq!(actions, ["activity_archived", "classroom_archived"]);
}

#[sqlx::test]
async fn a_conversation_started_while_deleting_is_still_counted(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let id: uuid::Uuid = published_activity_with_topic(&app, &a, "題")
        .await
        .parse()
        .unwrap();
    let student: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email = 'student@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    // 開始對話會先對活動列取 FOR SHARE；在它提交前發出刪除，刪除必須等它
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM activities WHERE id = $1 FOR SHARE")
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let deleting = tokio::spawn({
        let app = app.clone();
        let teacher = a.teacher.clone();
        async move {
            status_of(
                &app,
                "DELETE",
                &format!("/api/activities/{id}"),
                Some(&teacher),
                json!({"confirm_title": "題"}),
            )
            .await
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    sqlx::query(
        "INSERT INTO conversations (user_id, activity_id, classroom_id, title)
         SELECT $1, id, classroom_id, 't' FROM activities WHERE id = $2",
    )
    .bind(student)
    .bind(id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(deleting.await.unwrap(), StatusCode::NO_CONTENT);
    assert_eq!(count(&pool, "conversations").await, 0);
    assert_eq!(deleted_count(&pool, "activity_deleted").await, Some(1));
}

#[sqlx::test]
async fn the_conversation_list_can_be_narrowed_to_one_activity(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let topic = create_topic(&app, &a, "題").await;
    let first = publish_activity(&app, &a, "活動一", &topic).await;
    let second = publish_activity(&app, &a, "活動二", &topic).await;
    let conv = start_from_activity(&app, &a.student, &first).await;
    start_from_activity(&app, &a.student, &second).await;
    send(
        &app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/archive"),
            Some(&a.student),
            None,
        ),
    )
    .await;

    let list = json_body(
        send(
            &app,
            get(
                &format!("/api/conversations?archived=true&activity_id={first}&limit=1"),
                Some(&a.student),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert_eq!(list["items"][0]["id"], conv.as_str());
    // 另一個活動的對話沒有封存，不會出現在已封存檢視
    let list = json_body(
        send(
            &app,
            get(
                &format!("/api/conversations?archived=true&activity_id={second}"),
                Some(&a.student),
            ),
        )
        .await,
    )
    .await;
    assert!(list["items"].as_array().unwrap().is_empty());
    let res = send(
        &app,
        get("/api/conversations?activity_id=nope", Some(&a.student)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(res).await["error"]["code"], "invalid_activity_id");
}

#[sqlx::test]
async fn an_archived_classroom_must_be_restored_before_it_can_be_edited(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let path = format!("/api/classrooms/{}", a.classroom);
    status_of(
        &app,
        "PATCH",
        &path,
        Some(&a.teacher),
        json!({"is_archived": true}),
    )
    .await;

    // 還原與編輯不能在同一個請求（S-14 決定）
    let res = send(
        &app,
        json_req(
            "PATCH",
            &path,
            Some(&a.teacher),
            Some(json!({"is_archived": false, "name": "改名"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(res).await["error"]["code"], "classroom_archived");
    let (name, archived): (String, bool) =
        sqlx::query_as("SELECT name, is_archived FROM classrooms")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((name.as_str(), archived), ("測試教室", true));

    // 先還原，再編輯
    assert_eq!(
        status_of(
            &app,
            "PATCH",
            &path,
            Some(&a.teacher),
            json!({"is_archived": false})
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        status_of(
            &app,
            "PATCH",
            &path,
            Some(&a.teacher),
            json!({"name": "改名"})
        )
        .await,
        StatusCode::OK
    );
}

#[sqlx::test]
async fn deleting_a_student_account_ignores_classroom_archiving(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let activity = published_activity_with_topic(&app, &a, "題").await;
    start_from_activity(&app, &a.student, &activity).await;
    status_of(
        &app,
        "PATCH",
        &format!("/api/classrooms/{}", a.classroom),
        Some(&a.teacher),
        json!({"is_archived": true}),
    )
    .await;

    assert_eq!(
        status_of(
            &app,
            "DELETE",
            "/api/students/student@example.com",
            Some(&a.teacher),
            json!({"confirm_student_id": "10000000A"})
        )
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(count(&pool, "conversations").await, 0);
    assert_eq!(count(&pool, "enrollments").await, 0);
    // 教室本身仍是封存狀態
    let archived: bool = sqlx::query_scalar("SELECT is_archived FROM classrooms")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(archived);
}
