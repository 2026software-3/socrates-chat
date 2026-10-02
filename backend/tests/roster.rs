//! F-01.2 教師管理、F-01.3 角色、F-25 修課名單（S-01.3、S-01.4、S-08.2）。

mod common;

use axum::{Router, http::StatusCode};
use common::*;
use serde_json::json;
use socrates_chat_backend::roster::parse_roster;
use sqlx::PgPool;

async fn roles_of(app: &Router, cookie: &str) -> serde_json::Value {
    json_body(send(app, get("/api/me", Some(cookie))).await).await["roles"].clone()
}

async fn admin_cookie(app: &Router) -> String {
    login_as(app, "sub-admin", "admin@example.com").await
}

#[sqlx::test]
async fn plain_user_has_no_roles(pool: PgPool) {
    let app = test_app(pool);
    let c = login_as(&app, "sub-1", "nobody@example.com").await;
    let roles = roles_of(&app, &c).await;
    assert_eq!(
        roles,
        json!({"admin": false, "teacher": false, "student": false})
    );
}

#[sqlx::test]
async fn admin_adds_teacher_before_and_after_first_login(pool: PgPool) {
    let app = test_app(pool);
    let admin = admin_cookie(&app).await;

    // 對方尚未登入過：先加入，登入後即為教師
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(&admin),
            Some(json!({"email": " Teach@Example.com "})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let t = login_as(&app, "sub-t", "teach@example.com").await;
    assert_eq!(roles_of(&app, &t).await["teacher"], true);

    // 已登入過的帳號立即生效
    let other = login_as(&app, "sub-o", "other@example.com").await;
    assert_eq!(roles_of(&app, &other).await["teacher"], false);
    send(
        &app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(&admin),
            Some(json!({"email": "other@example.com"})),
        ),
    )
    .await;
    assert_eq!(roles_of(&app, &other).await["teacher"], true);

    // 重複加入不出錯；移除後立即失效
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(&admin),
            Some(json!({"email": "other@example.com"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = send(
        &app,
        json_req(
            "DELETE",
            "/api/admin/teachers/other@example.com",
            Some(&admin),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(roles_of(&app, &other).await["teacher"], false);

    let list = json_body(send(&app, get("/api/admin/teachers", Some(&admin))).await).await;
    assert_eq!(list["emails"], json!(["teach@example.com"]));
}

#[sqlx::test]
async fn teacher_management_is_admin_only(pool: PgPool) {
    let app = test_app(pool);
    let admin = admin_cookie(&app).await;
    send(
        &app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(&admin),
            Some(json!({"email": "t@example.com"})),
        ),
    )
    .await;
    let teacher = login_as(&app, "sub-t", "t@example.com").await;
    let student = login_as(&app, "sub-s", "s@example.com").await;

    for cookie in [Some(teacher.as_str()), Some(student.as_str()), None] {
        let expected = if cookie.is_some() {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::UNAUTHORIZED
        };
        let res = send(&app, get("/api/admin/teachers", cookie)).await;
        assert_eq!(res.status(), expected);
        let res = send(
            &app,
            json_req(
                "POST",
                "/api/admin/teachers",
                cookie,
                Some(json!({"email": "x@example.com"})),
            ),
        )
        .await;
        // 未登入時 CSRF 檢查通過（有 Origin），應為 401
        assert_eq!(res.status(), expected);
    }
}

#[sqlx::test]
async fn invalid_teacher_email_is_rejected(pool: PgPool) {
    let app = test_app(pool);
    let admin = admin_cookie(&app).await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(&admin),
            Some(json!({"email": "not-an-email"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(res).await["error"]["code"], "invalid_email");
}

#[sqlx::test]
async fn roster_import_reports_added_existing_and_invalid(pool: PgPool) {
    let app = test_app(pool);
    let admin = admin_cookie(&app).await;
    let body = json!({"text": "email\nA@x.com\n\nb@x.com\nbad-line\na@x.com\n"});
    let res = send(
        &app,
        json_req("POST", "/api/roster/import", Some(&admin), Some(body)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let r = json_body(res).await;
    assert_eq!(r["added"], 2);
    assert_eq!(r["existing"], 0);
    assert_eq!(r["invalid"], json!([{"line": 5, "value": "bad-line"}]));

    // 再匯入一次：全部已存在；匯入只新增，不移除
    let body = json!({"text": "b@x.com\nc@x.com"});
    let r = json_body(
        send(
            &app,
            json_req("POST", "/api/roster/import", Some(&admin), Some(body)),
        )
        .await,
    )
    .await;
    assert_eq!(r["added"], 1);
    assert_eq!(r["existing"], 1);
    let list = json_body(send(&app, get("/api/roster", Some(&admin))).await).await;
    assert_eq!(list["emails"], json!(["a@x.com", "b@x.com", "c@x.com"]));
}

#[sqlx::test]
async fn enrolled_student_gets_student_role_and_removal_is_immediate(pool: PgPool) {
    let app = test_app(pool);
    let admin = admin_cookie(&app).await;
    let s = login_as(&app, "sub-s", "s@x.com").await;
    assert_eq!(roles_of(&app, &s).await["student"], false);

    send(
        &app,
        json_req(
            "POST",
            "/api/roster/import",
            Some(&admin),
            Some(json!({"text": "s@x.com"})),
        ),
    )
    .await;
    assert_eq!(roles_of(&app, &s).await["student"], true);

    let res = send(
        &app,
        json_req("DELETE", "/api/roster/s@x.com", Some(&admin), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(roles_of(&app, &s).await["student"], false);

    // 重新加回即恢復
    send(
        &app,
        json_req(
            "POST",
            "/api/roster/import",
            Some(&admin),
            Some(json!({"text": "s@x.com"})),
        ),
    )
    .await;
    assert_eq!(roles_of(&app, &s).await["student"], true);

    let res = send(
        &app,
        json_req("DELETE", "/api/roster/ghost@x.com", Some(&admin), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn roster_management_requires_teacher_or_admin(pool: PgPool) {
    let app = test_app(pool);
    let admin = admin_cookie(&app).await;
    send(
        &app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(&admin),
            Some(json!({"email": "t@x.com"})),
        ),
    )
    .await;
    let teacher = login_as(&app, "sub-t", "t@x.com").await;
    send(
        &app,
        json_req(
            "POST",
            "/api/roster/import",
            Some(&admin),
            Some(json!({"text": "s@x.com"})),
        ),
    )
    .await;
    let student = login_as(&app, "sub-s", "s@x.com").await;
    let outsider = login_as(&app, "sub-o", "o@x.com").await;

    // 教師可以匯入
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/roster/import",
            Some(&teacher),
            Some(json!({"text": "n@x.com"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    // 學生、名單外帳號、未登入都不行
    for (cookie, expected) in [
        (Some(student.as_str()), StatusCode::FORBIDDEN),
        (Some(outsider.as_str()), StatusCode::FORBIDDEN),
        (None, StatusCode::UNAUTHORIZED),
    ] {
        let res = send(&app, get("/api/roster", cookie)).await;
        assert_eq!(res.status(), expected);
        let res = send(
            &app,
            json_req(
                "POST",
                "/api/roster/import",
                cookie,
                Some(json!({"text": "z@x.com"})),
            ),
        )
        .await;
        assert_eq!(res.status(), expected);
        let res = send(
            &app,
            json_req("DELETE", "/api/roster/s@x.com", cookie, None),
        )
        .await;
        assert_eq!(res.status(), expected);
    }
}

#[test]
fn parse_roster_handles_header_csv_columns_and_duplicates() {
    let p = parse_roster("Email,name\r\n A@x.com ,Alice\n\"b@x.com\"\nA@X.COM\nbroken@\n");
    assert_eq!(p.emails, vec!["a@x.com", "b@x.com"]);
    assert_eq!(p.invalid.len(), 1);
    assert_eq!(p.invalid[0].line, 5);
}
