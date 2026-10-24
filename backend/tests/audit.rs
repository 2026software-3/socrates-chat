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
    let (_teacher, room) = seed_classroom(&app, &admin).await;
    let import = |text: &'static str| json!({ "text": text });
    assert_eq!(
        post(
            &app,
            &format!("/api/classrooms/{room}/roster/import"),
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
            &format!("/api/classrooms/{room}/roster/import"),
            &admin,
            import("b@x.com\nc@x.com")
        )
        .await,
        StatusCode::OK
    );
    let res = send(
        &app,
        json_req(
            "DELETE",
            &format!("/api/classrooms/{room}/roster/a@x.com"),
            Some(&admin),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = send(
        &app,
        json_req(
            "DELETE",
            &format!("/api/classrooms/{room}/roster/a@x.com"),
            Some(&admin),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    let mut changes: Vec<_> = trail(&pool)
        .await
        .into_iter()
        .filter(|e| e.1.starts_with("roster_"))
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
            &format!("/api/classrooms/{}/roster/import", a.classroom),
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
        (
            &format!("/api/classrooms/{}/roster/import", a.classroom),
            json!({"text": "x@x.com"}),
        ),
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

/// 只在本測試的隔離資料庫拒絕指定動作，模擬稽核寫入失敗。
async fn reject_audit(pool: &PgPool, action: &str) {
    sqlx::query(
        "CREATE FUNCTION reject_test_audit() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
             IF NEW.action = TG_ARGV[0] THEN
                 RAISE EXCEPTION 'synthetic audit failure';
             END IF;
             RETURN NEW;
         END $$",
    )
    .execute(pool)
    .await
    .unwrap();
    let sql = match action {
        "roster_added" => {
            "CREATE TRIGGER reject_test_audit BEFORE INSERT ON audit_logs
                          FOR EACH ROW EXECUTE FUNCTION reject_test_audit('roster_added')"
        }
        "password_reset" => {
            "CREATE TRIGGER reject_test_audit BEFORE INSERT ON audit_logs
                            FOR EACH ROW EXECUTE FUNCTION reject_test_audit('password_reset')"
        }
        _ => panic!("unsupported test action"),
    };
    sqlx::query(sql).execute(pool).await.unwrap();
}

#[sqlx::test]
async fn roster_import_rolls_back_when_audit_fails_and_can_be_retried(pool: PgPool) {
    let app = test_app(pool.clone());
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    let (_teacher, room) = seed_classroom(&app, &admin).await;
    reject_audit(&pool, "roster_added").await;
    let body = json!({"text": "retry@example.com"});
    assert_eq!(
        post(
            &app,
            &format!("/api/classrooms/{room}/roster/import"),
            &admin,
            body.clone()
        )
        .await,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let enrolled: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM enrollments WHERE email = 'retry@example.com')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!enrolled, "失敗的匯入不得留下名單");
    sqlx::query("DROP TRIGGER reject_test_audit ON audit_logs")
        .execute(&pool)
        .await
        .unwrap();
    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/classrooms/{room}/roster/import"),
            Some(&admin),
            Some(body),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let result = json_body(res).await;
    assert_eq!(result["added"], 1);
    assert_eq!(result["existing"], 0);
    assert_eq!(result["credentials"][0]["email"], "retry@example.com");
    assert!(trail(&pool).await.contains(&entry(
        "admin@example.com",
        "roster_added",
        "retry@example.com"
    )));
}

#[sqlx::test]
async fn roster_import_rolls_back_batch_and_audits_when_account_creation_fails(pool: PgPool) {
    let app = test_app(pool.clone());
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    let (_teacher, room) = seed_classroom(&app, &admin).await;
    // 第二筆帳號建立失敗：第一筆名單、帳號與稽核也應回滾，避免遺失一次性的臨時密碼。
    sqlx::query(
        "ALTER TABLE users ADD CONSTRAINT reject_test_account
         CHECK (email <> 'fail@example.com') NOT VALID",
    )
    .execute(&pool)
    .await
    .unwrap();
    let before = trail(&pool).await;
    assert_eq!(
        post(
            &app,
            &format!("/api/classrooms/{room}/roster/import"),
            &admin,
            json!({"text": "first@example.com\nfail@example.com"})
        )
        .await,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    for sql in [
        "SELECT count(*) FROM users WHERE email IN ('first@example.com', 'fail@example.com')",
        "SELECT count(*) FROM enrollments WHERE email IN ('first@example.com', 'fail@example.com')",
    ] {
        let count: i64 = sqlx::query_scalar(sql).fetch_one(&pool).await.unwrap();
        assert_eq!(count, 0, "失敗的匯入不得留下名單或帳號資料");
    }
    assert_eq!(trail(&pool).await, before);
}

#[sqlx::test]
async fn password_reset_audit_failure_preserves_password_sessions_and_lock(pool: PgPool) {
    use socrates_chat_backend::{auth_store, password};
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let user_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email = 'student@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let old_hash = password::hash(&password::generate_temporary()).unwrap();
    auth_store::set_password(&pool, user_id, &old_hash, false)
        .await
        .unwrap();
    let now = chrono::Utc::now();
    for _ in 0..auth_store::MAX_LOGIN_FAILURES {
        auth_store::record_login_failure(&pool, "student@example.com", "", now)
            .await
            .unwrap();
    }
    let before = trail(&pool).await;
    reject_audit(&pool, "password_reset").await;
    assert_eq!(
        post(
            &app,
            "/api/admin/users/reset-password",
            &a.admin,
            json!({"email": "student@example.com"})
        )
        .await,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let (hash, must_change): (String, bool) =
        sqlx::query_as("SELECT password_hash, must_change_password FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(hash == old_hash, "失敗的重設必須保留原密碼");
    assert!(!must_change);
    assert!(
        auth_store::is_locked(&pool, "student@example.com", "", now)
            .await
            .unwrap()
    );
    assert_eq!(
        send(&app, get("/api/me", Some(&a.student))).await.status(),
        StatusCode::OK
    );
    assert_eq!(trail(&pool).await, before);
}

#[sqlx::test]
async fn password_reset_audit_failure_does_not_create_an_account(pool: PgPool) {
    let app = test_app(pool.clone());
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    reject_audit(&pool, "password_reset").await;
    assert_eq!(
        post(
            &app,
            "/api/admin/users/reset-password",
            &admin,
            json!({"email": "new@example.com"})
        )
        .await,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE email = 'new@example.com')")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!exists, "失敗的重設不得留下新帳號");
}
