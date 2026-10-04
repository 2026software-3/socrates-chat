//! F-01.4 帳號停用與復原、F-22.1 稽核日誌、F-22.5 刪除學生帳號：行為與授權（合成資料）。

mod common;

use axum::{
    Router,
    http::{Request, StatusCode},
};
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

async fn post(app: &Router, path: &str, cookie: Option<&str>, body: Option<Value>) -> StatusCode {
    send(app, json_req("POST", path, cookie, body))
        .await
        .status()
}

async fn students(app: &Router, cookie: &str) -> Value {
    json_body(send(app, get("/api/students", Some(cookie))).await).await
}

async fn audit_actions(pool: &PgPool) -> Vec<(String, String, Option<String>)> {
    sqlx::query_as("SELECT action, target_email, reason FROM audit_logs ORDER BY created_at, id")
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

#[sqlx::test]
async fn admin_disables_and_restores_an_account_with_audit_trail(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;

    let path = "/api/admin/users/student@example.com";
    assert_eq!(
        post(
            &app,
            &format!("{path}/disable"),
            Some(&a.admin),
            Some(json!({"reason": "  測試停用  "}))
        )
        .await,
        StatusCode::NO_CONTENT
    );

    // 立即失效：原本的 session 不能再用，並看到「帳號已停用」
    let res = send(&app, get("/api/me", Some(&a.student))).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "session 已被清除");
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

    let list = students(&app, &a.teacher).await;
    assert_eq!(list[0]["disabled"], true);

    // 復原後角色與資料照舊
    assert_eq!(
        post(&app, &format!("{path}/enable"), Some(&a.admin), None).await,
        StatusCode::NO_CONTENT
    );
    let cookie = login_as(&app, "sub-student", "student@example.com").await;
    let res = send(
        &app,
        get(&format!("/api/conversations/{conv}"), Some(&cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    assert_eq!(
        audit_actions(&pool).await,
        vec![
            (
                "account_disabled".to_string(),
                "student@example.com".to_string(),
                Some("測試停用".to_string())
            ),
            (
                "account_enabled".to_string(),
                "student@example.com".to_string(),
                None
            ),
        ]
    );
}

#[sqlx::test]
async fn disabled_account_cannot_use_password_login(pool: PgPool) {
    let app = test_app(pool);
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
    post(
        &app,
        "/api/admin/users/student@example.com/disable",
        Some(&a.admin),
        None,
    )
    .await;

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
async fn only_admins_can_disable_and_restore(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    for p in ["disable", "enable"] {
        let path = format!("/api/admin/users/student@example.com/{p}");
        for (cookie, status) in [
            (Some(a.teacher.as_str()), StatusCode::FORBIDDEN),
            (Some(a.student.as_str()), StatusCode::FORBIDDEN),
            (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
            (None, StatusCode::UNAUTHORIZED),
        ] {
            assert_eq!(post(&app, &path, cookie, None).await, status);
        }
    }
    // 沒被擋下來亂動：學生仍可使用
    assert_eq!(
        send(&app, get("/api/me", Some(&a.student))).await.status(),
        StatusCode::OK
    );
}

#[sqlx::test]
async fn cannot_disable_yourself_or_the_last_active_admin(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/users/admin@example.com/disable",
            Some(&a.admin),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(res).await["error"]["code"], "cannot_disable_self");

    // 第二位管理者可以被停用，但停用後唯一剩下的管理者受保護
    let admin2 = {
        let mut cfg = test_config();
        cfg.admin_emails.push("admin2@example.com".into());
        let app2 = test_app_with_config(pool.clone(), FakeAi::with(vec![]), cfg);
        login_as(&app2, "sub-admin2", "admin2@example.com").await
    };
    assert_eq!(
        post(
            &app,
            "/api/admin/users/admin2@example.com/disable",
            Some(&a.admin),
            None
        )
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(&app, get("/api/me", Some(&admin2))).await.status(),
        StatusCode::UNAUTHORIZED
    );
    // admin2 復原後，由 admin2 嘗試停用 admin：admin 還在，所以可以；但再停用 admin2 自己不行
    post(
        &app,
        "/api/admin/users/admin2@example.com/enable",
        Some(&a.admin),
        None,
    )
    .await;
    let admin2 = {
        let mut cfg = test_config();
        cfg.admin_emails.push("admin2@example.com".into());
        let app2 = test_app_with_config(pool, FakeAi::with(vec![]), cfg);
        login_as(&app2, "sub-admin2", "admin2@example.com").await
    };
    assert_eq!(
        post(
            &app,
            "/api/admin/users/admin@example.com/disable",
            Some(&admin2),
            None
        )
        .await,
        StatusCode::NO_CONTENT
    );
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/users/admin2@example.com/disable",
            Some(&admin2),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
}

#[sqlx::test]
async fn unknown_account_is_404_and_bad_email_is_400(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    assert_eq!(
        post(
            &app,
            "/api/admin/users/nobody@example.com/disable",
            Some(&a.admin),
            None
        )
        .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        post(
            &app,
            "/api/admin/users/not-an-email/disable",
            Some(&a.admin),
            None
        )
        .await,
        StatusCode::BAD_REQUEST
    );
}

#[sqlx::test]
async fn deleting_a_student_removes_account_data_and_roster_entry(pool: PgPool) {
    let ai = FakeAi::with(vec![]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    student_says(&app, &a, &conv, "我會拉桿").await;

    // 沒有再次確認（電子郵件不符）不能刪
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/students/student@example.com/delete",
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
            "POST",
            "/api/students/student@example.com/delete",
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
            "POST",
            "/api/students/student@example.com/delete",
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
            post(
                &app,
                "/api/students/student@example.com/delete",
                cookie,
                body("student@example.com")
            )
            .await,
            status
        );
    }
    // 教師與管理者帳號不是學生帳號，不能從這裡刪
    for email in ["teacher@example.com", "admin@example.com"] {
        let res = send(
            &app,
            json_req(
                "POST",
                &format!("/api/students/{email}/delete"),
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
