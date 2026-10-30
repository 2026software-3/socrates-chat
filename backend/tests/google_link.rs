//! 學生在個人設定連結 Google 帳號：信箱由 Google 證明是本人的才寫入，之後用 Google 登入會回到同一個帳號。

mod common;
use axum::{Router, body::Body, http::Request, http::Response, http::StatusCode};
use common::*;
use serde_json::json;
use sqlx::PgPool;

/// 教師匯入一位沒有信箱的學生，並以學號登入，回傳學生的 session cookie。
async fn emailless_student(app: &Router, a: &Actors, student_id: &str) -> String {
    let res = send(
        app,
        json_req(
            "POST",
            &format!("/api/classrooms/{}/roster/import", a.classroom),
            Some(&a.teacher),
            Some(json!({"text": format!("連結測試,{student_id}")})),
        ),
    )
    .await;
    let temp = json_body(res).await["credentials"][0]["temporary_password"]
        .as_str()
        .unwrap()
        .to_string();
    let res = send(
        app,
        json_req(
            "POST",
            "/api/auth/login",
            None,
            Some(json!({"account": student_id, "password": temp})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let temp_cookie = session_cookie(&res).unwrap();
    let new_password = format!("Fresh-{}", uuid::Uuid::new_v4());
    // 臨時密碼登入後必須先改密碼；改完用新密碼重新登入才是一般的學生 session
    let changed = send(
        app,
        json_req(
            "POST",
            "/api/auth/change-password",
            Some(&temp_cookie),
            Some(json!({"current_password": temp, "new_password": new_password})),
        ),
    )
    .await;
    assert!(changed.status().is_success());
    let res = send(
        app,
        json_req(
            "POST",
            "/api/auth/login",
            None,
            Some(json!({"account": student_id, "password": new_password})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    session_cookie(&res).unwrap()
}

/// 開始連結並回傳 state。
async fn start(app: &Router, cookie: &str) -> String {
    let res = send(app, get("/api/auth/google/link", Some(cookie))).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    location(&res).split("state=").nth(1).unwrap().to_string()
}

async fn finish(app: &Router, cookie: Option<&str>, code: &str, state: &str) -> Response<Body> {
    let mut b = Request::get(format!(
        "/api/auth/google/callback?code={code}&state={state}"
    ));
    if let Some(c) = cookie {
        b = b.header("cookie", c);
    }
    send(app, b.body(Body::empty()).unwrap()).await
}

async fn me(app: &Router, cookie: &str) -> serde_json::Value {
    json_body(send(app, get("/api/me", Some(cookie))).await).await
}

#[sqlx::test]
async fn only_a_signed_in_student_can_start_linking(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    assert_eq!(
        send(&app, get("/api/auth/google/link", None))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    for cookie in [&a.teacher, &a.admin] {
        assert_eq!(
            send(&app, get("/api/auth/google/link", Some(cookie)))
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
    }
}

#[sqlx::test]
async fn a_verified_google_email_is_attached_and_google_login_returns_to_the_same_account(
    pool: PgPool,
) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let cookie = emailless_student(&app, &a, "11234567A").await;
    let id = me(&app, &cookie).await["id"].clone();
    let state = start(&app, &cookie).await;
    let res = finish(&app, Some(&cookie), "ok:sub-link:Mine@Example.com", &state).await;
    assert_eq!(location(&res), format!("{APP_URL}/dashboard?link=ok"));
    let after = me(&app, &cookie).await;
    assert_eq!(after["email"], "mine@example.com");
    let google = login_as(&app, "sub-link", "mine@example.com").await;
    assert_eq!(me(&app, &google).await["id"], id);
}

#[sqlx::test]
async fn an_unverified_google_email_is_not_attached(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let cookie = emailless_student(&app, &a, "11234567A").await;
    let state = start(&app, &cookie).await;
    let res = finish(
        &app,
        Some(&cookie),
        "unverified:sub-x:x@example.com",
        &state,
    )
    .await;
    assert_eq!(
        location(&res),
        format!("{APP_URL}/dashboard?link_error=email_not_verified")
    );
    assert_eq!(me(&app, &cookie).await["email"], json!(null));
}

#[sqlx::test]
async fn an_email_that_belongs_to_another_account_is_refused(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let cookie = emailless_student(&app, &a, "11234567A").await;
    // 其他學生、教師與管理者的信箱（含 ADMIN_EMAILS）都不能綁到學號帳號
    for taken in [
        "student@example.com",
        "teacher@example.com",
        "admin@example.com",
    ] {
        let state = start(&app, &cookie).await;
        let res = finish(&app, Some(&cookie), &format!("ok:sub-t:{taken}"), &state).await;
        assert_eq!(
            location(&res),
            format!("{APP_URL}/dashboard?link_error=email_in_use"),
            "{taken}"
        );
    }
    assert_eq!(me(&app, &cookie).await["email"], json!(null));
}

#[sqlx::test]
async fn a_google_account_already_bound_elsewhere_is_refused(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let cookie = emailless_student(&app, &a, "11234567A").await;
    // 先用這個 Google 帳號登入過，產生了另一個（未開通）帳號
    login_as(&app, "sub-taken", "elsewhere@example.com").await;
    let state = start(&app, &cookie).await;
    let res = finish(
        &app,
        Some(&cookie),
        "ok:sub-taken:other@example.com",
        &state,
    )
    .await;
    assert_eq!(
        location(&res),
        format!("{APP_URL}/dashboard?link_error=email_in_use")
    );
}

#[sqlx::test]
async fn the_callback_must_come_from_the_same_signed_in_student(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let cookie = emailless_student(&app, &a, "11234567A").await;
    for other in [None, Some(a.student.as_str())] {
        let state = start(&app, &cookie).await;
        let res = finish(&app, other, "ok:sub-y:y@example.com", &state).await;
        assert_eq!(
            location(&res),
            format!("{APP_URL}/dashboard?link_error=invalid_state")
        );
    }
    assert_eq!(me(&app, &cookie).await["email"], json!(null));
    // state 單次使用
    let state = start(&app, &cookie).await;
    finish(&app, Some(&cookie), "ok:sub-y:y@example.com", &state).await;
    let again = finish(&app, Some(&cookie), "ok:sub-z:z@example.com", &state).await;
    // 用過的 state 已不存在，走一般登入的失敗路徑；不會再改信箱，也不會建立 session
    assert!(location(&again).contains("error=invalid_state"));
    assert!(session_cookie(&again).is_none());
    assert_eq!(me(&app, &cookie).await["email"], "y@example.com");
}

#[sqlx::test]
async fn linking_again_replaces_the_email_and_the_old_google_account_no_longer_signs_in(
    pool: PgPool,
) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let cookie = emailless_student(&app, &a, "11234567A").await;
    let id = me(&app, &cookie).await["id"].clone();
    let state = start(&app, &cookie).await;
    finish(&app, Some(&cookie), "ok:sub-old:old@example.com", &state).await;
    let state = start(&app, &cookie).await;
    let res = finish(&app, Some(&cookie), "ok:sub-new:new@example.com", &state).await;
    assert!(location(&res).ends_with("/dashboard?link=ok"));
    assert_eq!(me(&app, &cookie).await["email"], "new@example.com");
    let old = login_as(&app, "sub-old", "old@example.com").await;
    assert_ne!(me(&app, &old).await["id"], id);
    let new = login_as(&app, "sub-new", "new@example.com").await;
    assert_eq!(me(&app, &new).await["id"], id);
}

#[sqlx::test]
async fn unlinking_clears_the_email_and_the_student_id_still_signs_in(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let cookie = emailless_student(&app, &a, "11234567A").await;
    let state = start(&app, &cookie).await;
    finish(&app, Some(&cookie), "ok:sub-u:u@example.com", &state).await;
    let res = send(
        &app,
        json_req("DELETE", "/api/me/google-link", Some(&cookie), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(me(&app, &cookie).await["email"], json!(null));
    assert_eq!(
        send(&app, get("/api/me", Some(&cookie))).await.status(),
        StatusCode::OK
    );
    for c in [&a.teacher, &a.admin] {
        let res = send(
            &app,
            json_req("DELETE", "/api/me/google-link", Some(c), None),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }
    assert_eq!(
        send(&app, json_req("DELETE", "/api/me/google-link", None, None))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
}
