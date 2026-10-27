//! F-28.2 修課名單與成員歸屬教室（S-14、S-01.4）。

mod common;

use axum::{Router, http::StatusCode};
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

async fn import(
    app: &Router,
    cookie: Option<&str>,
    classroom: &str,
    text: &str,
) -> (StatusCode, Value) {
    let res = send(
        app,
        json_req(
            "POST",
            &format!("/api/classrooms/{classroom}/roster/import"),
            cookie,
            Some(json!({"text": fixture_roster(text)})),
        ),
    )
    .await;
    (res.status(), json_body(res).await)
}

async fn roster(app: &Router, cookie: Option<&str>, classroom: &str) -> (StatusCode, Value) {
    let res = send(
        app,
        get(&format!("/api/classrooms/{classroom}/roster"), cookie),
    )
    .await;
    (res.status(), json_body(res).await)
}

async fn remove(app: &Router, cookie: Option<&str>, classroom: &str, email: &str) -> StatusCode {
    send(
        app,
        json_req(
            "DELETE",
            &format!("/api/classrooms/{classroom}/roster/{email}"),
            cookie,
            None,
        ),
    )
    .await
    .status()
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

async fn student_flag(app: &Router, cookie: &str) -> bool {
    json_body(send(app, get("/api/me", Some(cookie))).await).await["roles"]["student"]
        .as_bool()
        .unwrap()
}

#[sqlx::test]
async fn roster_import_belongs_to_one_classroom_and_never_removes(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let room_b = create_classroom(&app, &a.teacher, "B 班").await;

    let (status, body) = import(
        &app,
        Some(&a.teacher),
        &a.classroom,
        "new@example.com\nstudent@example.com\nbroken",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["added"], 1);
    assert_eq!(body["existing"], 1);
    assert_eq!(
        body["invalid"],
        json!([{"line": 3, "reason": "invalid_format"}])
    );

    // 另一個教室不受影響
    let (_, other) = roster(&app, Some(&a.teacher), &room_b).await;
    assert_eq!(other["items"], json!([]));
    let (_, mine) = roster(&app, Some(&a.teacher), &a.classroom).await;
    let emails = mine["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["email"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        emails,
        std::collections::BTreeSet::from(["new@example.com", "student@example.com"])
    );

    // 只新增：再匯入一份不含舊人的名單，舊的仍在
    let (_, body) = import(&app, Some(&a.teacher), &a.classroom, "third@example.com").await;
    assert_eq!(body["added"], 1);
    let (_, mine) = roster(&app, Some(&a.teacher), &a.classroom).await;
    assert_eq!(mine["items"].as_array().unwrap().len(), 3);
}

#[sqlx::test]
async fn same_email_in_several_classrooms_is_one_account_with_student_access(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let room_b = create_classroom(&app, &a.teacher, "B 班").await;

    let (status, body) = import(&app, Some(&a.teacher), &room_b, "student@example.com").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["added"], 1, "對這間教室來說是新加入");

    let accounts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM users WHERE email = 'student@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(accounts, 1);
    assert!(student_flag(&app, &a.student).await);

    // 兩個教室的學生列表都有這個帳號，且是同一個 id
    let ids: Vec<Value> = futures_util::future::join_all([&a.classroom, &room_b].map(|room| {
        let app = app.clone();
        let cookie = a.teacher.clone();
        let room = room.clone();
        async move {
            let res = send(
                &app,
                get(&format!("/api/classrooms/{room}/students"), Some(&cookie)),
            )
            .await;
            assert_eq!(res.status(), StatusCode::OK);
            json_body(res).await["items"][0]["id"].clone()
        }
    }))
    .await;
    assert!(ids[0].is_string());
    assert_eq!(ids[0], ids[1]);
}

#[sqlx::test]
async fn removal_only_affects_that_classroom_and_keeps_data_until_re_added(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let room_b = create_classroom(&app, &a.teacher, "B 班").await;
    // 已登入但不在任何名單的帳號：匯入前不是學生，匯入後立即是學生
    assert!(!student_flag(&app, &a.outsider).await);
    import(&app, Some(&a.teacher), &room_b, "outsider@example.com").await;
    assert!(student_flag(&app, &a.outsider).await);

    import(&app, Some(&a.teacher), &room_b, "student@example.com").await;
    let conv = start_conversation(&app, &a).await;

    // 移出 A：仍在 B，保有學生權限與資料
    assert_eq!(
        remove(&app, Some(&a.teacher), &a.classroom, "student@example.com").await,
        StatusCode::NO_CONTENT
    );
    assert!(student_flag(&app, &a.student).await);

    // 兩邊都移出：立即失去學生權限，看到尚未開通
    assert_eq!(
        remove(&app, Some(&a.teacher), &room_b, "student@example.com").await,
        StatusCode::NO_CONTENT
    );
    assert!(!student_flag(&app, &a.student).await);
    let res = send(&app, get("/api/conversations", Some(&a.student))).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(res).await["error"]["code"], "not_enrolled");

    // 再移一次：不在名單內
    assert_eq!(
        remove(&app, Some(&a.teacher), &room_b, "student@example.com").await,
        StatusCode::NOT_FOUND
    );

    // 重新加回：恢復存取，資料照舊
    import(&app, Some(&a.teacher), &a.classroom, "student@example.com").await;
    assert!(student_flag(&app, &a.student).await);
    let list = json_body(send(&app, get("/api/conversations", Some(&a.student))).await).await;
    assert_eq!(list["items"][0]["id"], conv);
}

#[sqlx::test]
async fn other_teachers_cannot_touch_someone_elses_classroom_roster(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let intruder = add_teacher(&app, &a.admin, "sub-t2", "teacher2@example.com").await;
    let fake = "00000000-0000-4000-8000-000000000000";

    // 他人的教室與不存在的教室一律相同的 404，不外露是否存在
    let mut seen = vec![];
    for room in [a.classroom.as_str(), fake, "not-a-uuid"] {
        let (s1, b1) = roster(&app, Some(&intruder), room).await;
        let (s2, b2) = import(&app, Some(&intruder), room, "x@example.com").await;
        let s3 = remove(&app, Some(&intruder), room, "student@example.com").await;
        let res = send(
            &app,
            get(&format!("/api/classrooms/{room}/students"), Some(&intruder)),
        )
        .await;
        let s4 = res.status();
        let b4 = json_body(res).await;
        assert_eq!(
            [s1, s2, s3, s4],
            [StatusCode::NOT_FOUND; 4],
            "classroom {room}"
        );
        seen.push([b1, b2, b4].map(|b| b["error"].get("code").cloned()));
    }
    assert!(
        seen.windows(2).all(|w| w[0] == w[1]),
        "responses must not reveal existence"
    );

    // 沒有任何副作用
    let (_, mine) = roster(&app, Some(&a.teacher), &a.classroom).await;
    assert_eq!(mine["items"].as_array().unwrap().len(), 1);
    assert_eq!(mine["items"][0]["email"], "student@example.com");
}

#[sqlx::test]
async fn roster_endpoints_require_teacher_or_admin(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let room = &a.classroom;

    for (name, cookie, expected) in [
        ("anonymous", None, StatusCode::UNAUTHORIZED),
        ("student", Some(a.student.as_str()), StatusCode::FORBIDDEN),
        ("outsider", Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
    ] {
        let (s, _) = roster(&app, cookie, room).await;
        assert_eq!(s, expected, "list as {name}");
        let (s, _) = import(&app, cookie, room, "x@example.com").await;
        assert_eq!(s, expected, "import as {name}");
        assert_eq!(
            remove(&app, cookie, room, "student@example.com").await,
            expected,
            "remove as {name}"
        );
        let res = send(
            &app,
            get(&format!("/api/classrooms/{room}/students"), cookie),
        )
        .await;
        assert_eq!(res.status(), expected, "students as {name}");
    }
    let (_, mine) = roster(&app, Some(&a.teacher), room).await;
    assert_eq!(mine["items"].as_array().unwrap().len(), 1);
    assert_eq!(mine["items"][0]["email"], "student@example.com");
}

#[sqlx::test]
async fn admin_and_teacher_get_credentials_for_new_students(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;

    let (status, body) = import(&app, Some(&a.admin), &a.classroom, "fresh@example.com").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["added"], 1);
    assert_eq!(body["credentials"].as_array().unwrap().len(), 1);
    assert!(body["credentials"][0]["student_id"].is_string());

    let (_, body) = import(&app, Some(&a.teacher), &a.classroom, "other@example.com").await;
    assert_eq!(body["credentials"].as_array().unwrap().len(), 1);

    assert_eq!(
        remove(&app, Some(&a.admin), &a.classroom, "fresh@example.com").await,
        StatusCode::NO_CONTENT
    );
}

#[sqlx::test]
async fn class_dashboard_student_total_counts_distinct_students_in_own_classrooms(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let room_b = create_classroom(&app, &a.teacher, "B 班").await;
    import(
        &app,
        Some(&a.teacher),
        &room_b,
        "student@example.com\nb@example.com",
    )
    .await;
    let other = add_teacher(&app, &a.admin, "sub-t2", "teacher2@example.com").await;
    let room_c = create_classroom(&app, &other, "C 班").await;
    import(
        &app,
        Some(&other),
        &room_c,
        "c1@example.com\nc2@example.com\nc3@example.com",
    )
    .await;

    let mine = json_body(send(&app, get("/api/dashboard/class", Some(&a.teacher))).await).await;
    assert_eq!(mine["students_total"], 2);
    let theirs = json_body(send(&app, get("/api/dashboard/class", Some(&other))).await).await;
    assert_eq!(theirs["students_total"], 3);
    let all = json_body(send(&app, get("/api/dashboard/class", Some(&a.admin))).await).await;
    assert_eq!(all["students_total"], 5);
}

#[sqlx::test]
async fn deleting_a_student_account_removes_every_classroom_membership(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let room_b = create_classroom(&app, &a.teacher, "B 班").await;
    import(&app, Some(&a.teacher), &room_b, "student@example.com").await;

    let res = send(
        &app,
        json_req(
            "DELETE",
            "/api/students/student@example.com",
            Some(&a.teacher),
            Some(json!({"confirm_student_id": "10000000A"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM enrollments")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
}
