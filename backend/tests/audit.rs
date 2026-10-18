//! F-22.1 稽核日誌：登入、教師異動、名單異動與重設密碼都要留下紀錄（S-01.3、S-02.5）。
//! 只記錄操作者、動作與對象；全部使用合成資料。

mod common;

use axum::{Router, http::StatusCode};
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

/// (操作者信箱, 動作, 對象信箱)，依時間排序。
async fn trail(pool: &PgPool) -> Vec<(Option<String>, String, String)> {
    sqlx::query_as(
        "SELECT u.email, a.action, a.target_email
         FROM audit_logs a LEFT JOIN users u ON u.id = a.actor_id
         ORDER BY a.created_at, a.id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

fn entry(actor: &str, action: &str, target: &str) -> (Option<String>, String, String) {
    (Some(actor.into()), action.into(), target.into())
}

async fn post(app: &Router, path: &str, cookie: &str, body: Value) -> StatusCode {
    send(app, json_req("POST", path, Some(cookie), Some(body)))
        .await
        .status()
}

#[sqlx::test]
async fn google_login_is_recorded_only_when_it_succeeds(pool: PgPool) {
    let app = test_app(pool.clone());
    login_as(&app, "sub-1", "someone@example.com").await;
    assert_eq!(
        trail(&pool).await,
        vec![entry("someone@example.com", "login", "someone@example.com")]
    );

    // 失敗的登入（信箱未驗證）不寫入
    let res = send(
        &app,
        axum::http::Request::get("/api/auth/google/login")
            .body(axum::body::Body::empty())
            .unwrap(),
    )
    .await;
    let state = location(&res).split("state=").nth(1).unwrap().to_string();
    send(
        &app,
        axum::http::Request::get(format!(
            "/api/auth/google/callback?code=unverified:sub-2:other@example.com&state={state}"
        ))
        .body(axum::body::Body::empty())
        .unwrap(),
    )
    .await;
    assert_eq!(trail(&pool).await.len(), 1);
}

#[sqlx::test]
async fn password_login_is_recorded_only_when_it_succeeds(pool: PgPool) {
    let app = test_app(pool.clone());
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/users/reset-password",
            Some(&admin),
            Some(json!({"email": "pw@example.com"})),
        ),
    )
    .await;
    let temp = json_body(res).await["temporary_password"]
        .as_str()
        .unwrap()
        .to_string();
    let login = |pw: String| {
        let app = app.clone();
        async move {
            send(
                &app,
                json_req(
                    "POST",
                    "/api/auth/login",
                    None,
                    Some(json!({"email": "pw@example.com", "password": pw})),
                ),
            )
            .await
            .status()
        }
    };
    assert_eq!(
        login("wrong-password-1".into()).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(login(temp).await, StatusCode::OK);
    let logins: Vec<_> = trail(&pool)
        .await
        .into_iter()
        .filter(|e| e.1 == "login" && e.2 == "pw@example.com")
        .collect();
    assert_eq!(
        logins,
        vec![entry("pw@example.com", "login", "pw@example.com")]
    );
}

#[sqlx::test]
async fn teacher_changes_are_recorded_and_idempotent_calls_are_not_repeated(pool: PgPool) {
    let app = test_app(pool.clone());
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    let add = json!({"email": " Teach@Example.com "});
    for _ in 0..2 {
        assert_eq!(
            post(&app, "/api/admin/teachers", &admin, add.clone()).await,
            StatusCode::NO_CONTENT
        );
    }
    let res = send(
        &app,
        json_req(
            "DELETE",
            "/api/admin/teachers/teach@example.com",
            Some(&admin),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    // 移除不存在的教師：404，不寫紀錄
    let res = send(
        &app,
        json_req(
            "DELETE",
            "/api/admin/teachers/teach@example.com",
            Some(&admin),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    let changes: Vec<_> = trail(&pool)
        .await
        .into_iter()
        .filter(|e| e.1 != "login")
        .collect();
    assert_eq!(
        changes,
        vec![
            entry("admin@example.com", "teacher_added", "teach@example.com"),
            entry("admin@example.com", "teacher_removed", "teach@example.com"),
        ]
    );
}

#[sqlx::test]
async fn roster_changes_are_recorded_per_student_and_skip_existing_entries(pool: PgPool) {
    let app = test_app(pool.clone());
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    let import = |text: &'static str| json!({ "text": text });
    assert_eq!(
        post(
            &app,
            "/api/roster/import",
            &admin,
            import("a@x.com\nb@x.com\nbad-line")
        )
        .await,
        StatusCode::OK
    );
    // 已在名單上的人不再寫紀錄
    assert_eq!(
        post(
            &app,
            "/api/roster/import",
            &admin,
            import("b@x.com\nc@x.com")
        )
        .await,
        StatusCode::OK
    );
    let res = send(
        &app,
        json_req("DELETE", "/api/roster/a@x.com", Some(&admin), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = send(
        &app,
        json_req("DELETE", "/api/roster/a@x.com", Some(&admin), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    let mut changes: Vec<_> = trail(&pool)
        .await
        .into_iter()
        .filter(|e| e.1 != "login")
        .map(|e| (e.1, e.2))
        .collect();
    let removed = changes.pop().unwrap();
    changes.sort();
    assert_eq!(
        removed,
        ("roster_removed".to_string(), "a@x.com".to_string())
    );
    assert_eq!(
        changes,
        vec![
            ("roster_added".to_string(), "a@x.com".to_string()),
            ("roster_added".to_string(), "b@x.com".to_string()),
            ("roster_added".to_string(), "c@x.com".to_string()),
        ]
    );
}

#[sqlx::test]
async fn teacher_roster_changes_name_the_teacher_as_actor(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    assert_eq!(
        post(
            &app,
            "/api/roster/import",
            &a.teacher,
            json!({"text": "new@x.com"})
        )
        .await,
        StatusCode::OK
    );
    assert!(trail(&pool).await.contains(&entry(
        "teacher@example.com",
        "roster_added",
        "new@x.com"
    )));
}

#[sqlx::test]
async fn password_reset_is_recorded_without_the_password(pool: PgPool) {
    let app = test_app(pool.clone());
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/users/reset-password",
            Some(&admin),
            Some(json!({"email": "Target@Example.com"})),
        ),
    )
    .await;
    let temp = json_body(res).await["temporary_password"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(trail(&pool).await.contains(&entry(
        "admin@example.com",
        "password_reset",
        "target@example.com"
    )));
    let leaked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs
         WHERE target_email = $1 OR action = $1 OR coalesce(reason, '') = $1",
    )
    .bind(&temp)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(leaked, 0);
}

#[sqlx::test]
async fn rejected_requests_leave_no_audit_entries(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let before = trail(&pool).await.len();
    let attempts = [
        ("/api/admin/teachers", json!({"email": "x@x.com"})),
        (
            "/api/admin/users/reset-password",
            json!({"email": "x@x.com"}),
        ),
        ("/api/roster/import", json!({"text": "x@x.com"})),
    ];
    for (path, body) in attempts {
        for cookie in [&a.student, &a.outsider] {
            let s = post(&app, path, cookie, body.clone()).await;
            assert_eq!(s, StatusCode::FORBIDDEN, "{path}");
        }
    }
    // 教師不能做管理者專屬的操作，也沒有登入
    assert_eq!(
        post(
            &app,
            "/api/admin/teachers",
            &a.teacher,
            json!({"email": "x@x.com"})
        )
        .await,
        StatusCode::FORBIDDEN
    );
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/teachers",
            None,
            Some(json!({"email": "x@x.com"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(trail(&pool).await.len(), before);
}
