//! F-01.4 帳號停用與復原、F-22.1 稽核日誌、F-22.5 刪除學生帳號：行為與授權（合成資料）。

mod common;

use axum::{Router, http::StatusCode};
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

async fn post(app: &Router, path: &str, cookie: Option<&str>, body: Option<Value>) -> StatusCode {
    send(app, json_req("POST", path, cookie, body))
        .await
        .status()
}

/// 第一頁的學生（測試資料遠少於預設分頁大小）。
async fn students(app: &Router, cookie: &str) -> Value {
    json_body(send(app, get("/api/students", Some(cookie))).await).await["items"].take()
}

async fn audit_actions(pool: &PgPool) -> Vec<(String, String, Option<String>)> {
    sqlx::query_as("SELECT action, target_email, reason FROM audit_logs
         WHERE action IN ('account_disabled', 'account_enabled', 'student_deleted') ORDER BY created_at, id")
        .fetch_all(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn student_list_shows_account_state_to_teachers_and_admins_only(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    // 另一位還沒登入過的名單學生
    post(
        &app,
        "/api/roster/import",
        Some(&a.teacher),
        Some(json!({"text": "later@example.com"})),
    )
    .await;

    for cookie in [&a.teacher, &a.admin] {
        let list = students(&app, cookie).await;
        let rows = list.as_array().unwrap();
        let by = |e: &str| rows.iter().find(|r| r["email"] == e).unwrap().clone();
        assert_eq!(by("student@example.com")["has_account"], true);
        assert_eq!(by("student@example.com")["disabled"], false);
        assert_eq!(by("later@example.com")["has_account"], false);
        assert_eq!(by("later@example.com")["completed_conversations"], 0);
    }
    for (cookie, status) in [
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        (None, StatusCode::UNAUTHORIZED),
    ] {
        let res = send(&app, get("/api/students", cookie)).await;
        assert_eq!(res.status(), status);
    }
}

async fn user_id(pool: &PgPool, email: &str) -> uuid::Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE email=$1")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn status_put_is_idempotent_revokes_sessions_and_audits(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    let extra = login_as(&app, "sub-student", "student@example.com").await;
    let id = user_id(&pool, "student@example.com").await;
    let path = format!("/api/admin/users/{id}/status");
    for disabled in [true, true, false, false] {
        let res = send(
            &app,
            json_req(
                "PUT",
                &path,
                Some(&a.admin),
                Some(json!({"disabled":disabled,"reason":" synthetic "})),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        if disabled {
            for cookie in [&a.student, &extra] {
                assert_eq!(
                    send(&app, get("/api/me", Some(cookie))).await.status(),
                    StatusCode::UNAUTHORIZED
                );
            }
        }
    }
    let actions = audit_actions(&pool).await;
    assert_eq!(actions.len(), 2);
    assert_eq!(
        actions[0],
        (
            "account_disabled".into(),
            "student@example.com".into(),
            Some("synthetic".into())
        )
    );
    assert_eq!(actions[1].0, "account_enabled");
    let cookie = login_as(&app, "sub-student", "student@example.com").await;
    assert_eq!(
        send(
            &app,
            get(&format!("/api/conversations/{conv}"), Some(&cookie))
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(students(&app, &a.admin).await[0]["id"], id.to_string());
    for suffix in ["disable", "enable"] {
        assert_eq!(
            post(
                &app,
                &format!("/api/admin/users/student@example.com/{suffix}"),
                Some(&a.admin),
                None
            )
            .await,
            StatusCode::NOT_FOUND
        );
    }
}

#[sqlx::test]
async fn status_put_requires_admin_and_existing_uuid(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let id = user_id(&pool, "student@example.com").await;
    for disabled in [true, false] {
        for (cookie, status) in [
            (None, StatusCode::UNAUTHORIZED),
            (Some(a.teacher.as_str()), StatusCode::FORBIDDEN),
            (Some(a.student.as_str()), StatusCode::FORBIDDEN),
            (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        ] {
            let res = send(
                &app,
                json_req(
                    "PUT",
                    &format!("/api/admin/users/{id}/status"),
                    cookie,
                    Some(json!({"disabled":disabled})),
                ),
            )
            .await;
            assert_eq!(res.status(), status);
        }
        assert_eq!(
            send(
                &app,
                json_req(
                    "PUT",
                    &format!("/api/admin/users/{}/status", uuid::Uuid::new_v4()),
                    Some(&a.admin),
                    Some(json!({"disabled":disabled}))
                )
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
    }
    assert!(audit_actions(&pool).await.is_empty());
    assert_eq!(
        send(
            &app,
            json_req(
                "PUT",
                &format!("/api/admin/users/{id}/status"),
                Some(&a.admin),
                Some(json!({}))
            )
        )
        .await
        .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[sqlx::test]
async fn status_put_preserves_admin_safety_under_concurrency(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let id = user_id(&pool, "admin@example.com").await;
    let res = send(
        &app,
        json_req(
            "PUT",
            &format!("/api/admin/users/{id}/status"),
            Some(&a.admin),
            Some(json!({"disabled":true})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(res).await["error"]["code"], "cannot_disable_self");
    let mut cfg = test_config();
    cfg.admin_emails.push("admin2@example.com".into());
    let app2 = test_app_with_config(pool.clone(), FakeAi::with(vec![]), cfg);
    let admin2 = login_as(&app2, "sub-admin2", "admin2@example.com").await;
    let id2 = user_id(&pool, "admin2@example.com").await;
    let req1 = json_req(
        "PUT",
        &format!("/api/admin/users/{id2}/status"),
        Some(&a.admin),
        Some(json!({"disabled":true})),
    );
    let req2 = json_req(
        "PUT",
        &format!("/api/admin/users/{id}/status"),
        Some(&admin2),
        Some(json!({"disabled":true})),
    );
    let (r1, r2) = tokio::join!(send(&app, req1), send(&app, req2));
    let statuses = [r1.status(), r2.status()];
    assert!(statuses.contains(&StatusCode::NO_CONTENT));
    assert!(statuses.iter().all(|s| {
        [
            StatusCode::NO_CONTENT,
            StatusCode::CONFLICT,
            StatusCode::FORBIDDEN,
            StatusCode::UNAUTHORIZED,
        ]
        .contains(s)
    }));
    let active: i64 =
        sqlx::query_scalar("SELECT count(*) FROM users WHERE is_admin AND disabled_at IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(active, 1);
}

#[sqlx::test]
async fn disabled_account_cannot_use_google_login(pool: PgPool) {
    use axum::http::Request;
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let id = user_id(&pool, "student@example.com").await;
    assert_eq!(
        send(
            &app,
            json_req(
                "PUT",
                &format!("/api/admin/users/{id}/status"),
                Some(&a.admin),
                Some(json!({"disabled":true}))
            )
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let res = send(
        &app,
        Request::get("/api/auth/google/login")
            .body(axum::body::Body::empty())
            .unwrap(),
    )
    .await;
    let state = location(&res).split("state=").nth(1).unwrap().to_string();
    let res = send(
        &app,
        Request::get(format!(
            "/api/auth/google/callback?code=ok:sub-student:student@example.com&state={state}"
        ))
        .body(axum::body::Body::empty())
        .unwrap(),
    )
    .await;
    assert!(location(&res).ends_with("/login?error=account_disabled"));
    assert!(session_cookie(&res).is_none());
}

#[sqlx::test]
async fn status_audit_failure_rolls_back_account_and_sessions(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let id = user_id(&pool, "student@example.com").await;
    sqlx::raw_sql("CREATE FUNCTION reject_status_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'synthetic failure'; END $$; CREATE TRIGGER reject_status_audit BEFORE INSERT ON audit_logs FOR EACH ROW EXECUTE FUNCTION reject_status_audit();").execute(&pool).await.unwrap();
    let res = send(
        &app,
        json_req(
            "PUT",
            &format!("/api/admin/users/{id}/status"),
            Some(&a.admin),
            Some(json!({"disabled":true})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json_body(res).await["error"]["code"], "internal");
    assert_eq!(
        send(&app, get("/api/me", Some(&a.student))).await.status(),
        StatusCode::OK
    );
    let disabled: bool =
        sqlx::query_scalar("SELECT disabled_at IS NOT NULL FROM users WHERE id=$1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!disabled);
}

#[sqlx::test]
async fn disabled_account_cannot_use_password_login(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/users/reset-password",
            Some(&a.admin),
            Some(json!({"email": "student@example.com"})),
        ),
    )
    .await;
    let temp = json_body(res).await["temporary_password"]
        .as_str()
        .unwrap()
        .to_string();
    let id = user_id(&pool, "student@example.com").await;
    assert_eq!(
        send(
            &app,
            json_req(
                "PUT",
                &format!("/api/admin/users/{id}/status"),
                Some(&a.admin),
                Some(json!({"disabled":true}))
            )
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );

    let login = |pw: String| {
        send(
            &app,
            json_req(
                "POST",
                "/api/auth/login",
                None,
                Some(json!({"email": "student@example.com", "password": pw})),
            ),
        )
    };
    let res = login(temp).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(res).await["error"]["code"], "account_disabled");
    // 密碼不對時不揭露帳號狀態
    let res = login("wrong-password".into()).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn deleting_a_student_removes_account_data_and_roster_entry(pool: PgPool) {
    let ai = FakeAi::with(vec![]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "我會拉桿").await;
    sqlx::query("INSERT INTO summaries (conversation_id) VALUES ($1)")
        .bind(uuid::Uuid::parse_str(&conv).unwrap())
        .execute(&pool)
        .await
        .unwrap();

    // 沒有再次確認（電子郵件不符）不能刪
    let res = send(
        &app,
        json_req(
            "DELETE",
            "/api/students/student@example.com",
            Some(&a.teacher),
            Some(json!({"confirm_email": "other@example.com"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(res).await["error"]["code"],
        "confirmation_mismatch"
    );
    assert_eq!(
        students(&app, &a.teacher).await.as_array().unwrap().len(),
        1
    );

    let res = send(
        &app,
        json_req(
            "DELETE",
            "/api/students/student@example.com",
            Some(&a.teacher),
            Some(json!({"confirm_email": " Student@Example.com "})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    assert!(
        students(&app, &a.teacher)
            .await
            .as_array()
            .unwrap()
            .is_empty()
    );
    for sql in [
        "SELECT count(*) FROM users WHERE email = 'student@example.com'",
        "SELECT count(*) FROM conversations",
        "SELECT count(*) FROM messages",
        "SELECT count(*) FROM summaries",
    ] {
        let (n,): (i64,) = sqlx::query_as(sql).fetch_one(&pool).await.unwrap();
        assert_eq!(n, 0, "{sql}");
    }
    assert_eq!(
        send(&app, get("/api/me", Some(&a.student))).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        audit_actions(&pool).await,
        vec![(
            "student_deleted".to_string(),
            "student@example.com".to_string(),
            None
        )]
    );

    // 再刪一次：已不存在
    let res = send(
        &app,
        json_req(
            "DELETE",
            "/api/students/student@example.com",
            Some(&a.admin),
            Some(json!({"confirm_email": "student@example.com"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn delete_is_limited_to_teachers_and_never_removes_staff(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let body = |e: &str| Some(json!({"confirm_email": e}));
    for (cookie, status) in [
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        (None, StatusCode::UNAUTHORIZED),
    ] {
        assert_eq!(
            send(
                &app,
                json_req(
                    "DELETE",
                    "/api/students/student@example.com",
                    cookie,
                    body("student@example.com")
                )
            )
            .await
            .status(),
            status
        );
    }
    // 教師與管理者帳號不是學生帳號，不能從這裡刪
    for email in ["teacher@example.com", "admin@example.com"] {
        let res = send(
            &app,
            json_req(
                "DELETE",
                &format!("/api/students/{email}"),
                Some(&a.admin),
                body(email),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::CONFLICT, "{email}");
        assert_eq!(json_body(res).await["error"]["code"], "not_a_student");
    }
    assert_eq!(
        send(&app, get("/api/me", Some(&a.teacher))).await.status(),
        StatusCode::OK
    );
}

#[sqlx::test]
async fn legacy_student_delete_route_is_removed(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let status = post(
        &app,
        "/api/students/student@example.com/delete",
        Some(&a.teacher),
        Some(json!({"confirm_email": "student@example.com"})),
    )
    .await;
    assert!(matches!(
        status,
        StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
    ));
    assert_eq!(
        students(&app, &a.teacher).await.as_array().unwrap().len(),
        1
    );
    assert!(audit_actions(&pool).await.is_empty());
}
