//! 教室名單、學生與活動列表的 keyset 分頁（#286–#288）。資料皆為合成資料。
mod common;
use axum::{Router, http::StatusCode};
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// 依 next_cursor 走完所有頁，回傳每頁內容。
async fn all_pages(app: &Router, base: &str, cookie: &str) -> Vec<Value> {
    let sep = if base.contains('?') { '&' } else { '?' };
    let mut path = base.to_string();
    let mut pages = Vec::new();
    loop {
        let res = send(app, get(&path, Some(cookie))).await;
        assert_eq!(res.status(), StatusCode::OK, "{path}");
        let page = json_body(res).await;
        let next = page["next_cursor"].as_str().map(str::to_string);
        pages.push(page);
        assert!(pages.len() <= 30, "too many pages");
        match next {
            Some(cursor) => path = format!("{base}{sep}cursor={cursor}"),
            None => break,
        }
    }
    pages
}

fn field(pages: &[Value], name: &str) -> Vec<String> {
    pages
        .iter()
        .flat_map(|p| p["items"].as_array().unwrap().iter())
        .map(|r| r[name].as_str().unwrap().to_string())
        .collect()
}

/// 把 `n` 位合成學生加進教室名單（`s01@example.com` …），回傳他們的電子郵件（已排序）。
async fn enroll(app: &Router, a: &Actors, n: usize) -> Vec<String> {
    let emails: Vec<String> = (1..=n).map(|i| format!("s{i:02}@example.com")).collect();
    let res = import_roster(app, &a.admin, &a.classroom, &emails.join("\n")).await;
    assert!(res.status().is_success());
    emails
}

async fn error_code(app: &Router, path: &str, cookie: &str) -> (StatusCode, String) {
    let res = send(app, get(path, Some(cookie))).await;
    let status = res.status();
    let code = json_body(res).await["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    (status, code)
}

// ---- GET /api/classrooms/{id}/students ----

#[sqlx::test]
async fn students_paginate_by_email(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let mut want = enroll(&app, &a, 5).await;
    want.push("student@example.com".into());
    want.sort();

    let base = format!("/api/classrooms/{}/students?limit=2", a.classroom);
    let pages = all_pages(&app, &base, &a.teacher).await;
    assert_eq!(pages.len(), 3);
    assert_eq!(field(&pages, "email"), want);
    assert_eq!(pages[2]["next_cursor"], Value::Null);
}

#[sqlx::test]
async fn students_items_only_carry_allowed_fields(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let res = send(
        &app,
        get(
            &format!("/api/classrooms/{}/students", a.classroom),
            Some(&a.teacher),
        ),
    )
    .await;
    let page = json_body(res).await;
    let mut keys: Vec<&str> = page["items"][0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "completed_conversations",
            "disabled",
            "display_name",
            "email",
            "has_account",
            "id"
        ]
    );
    assert_eq!(page.as_object().unwrap().len(), 2);
}

#[sqlx::test]
async fn students_validate_input(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let base = format!("/api/classrooms/{}/students", a.classroom);
    for (query, code) in [
        ("limit=0", "invalid_limit"),
        ("limit=201", "invalid_limit"),
        ("limit=-1", "invalid_limit"),
        ("limit=x", "invalid_limit"),
        ("limit=", "invalid_limit"),
        ("cursor=broken", "invalid_cursor"),
        ("cursor=", "invalid_cursor"),
        ("cursor=e30", "invalid_cursor"),
    ] {
        let (status, got) = error_code(&app, &format!("{base}?{query}"), &a.teacher).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(got, code, "{query}");
    }
    // 上限 200 的頁面大小可用
    let res = send(&app, get(&format!("{base}?limit=200"), Some(&a.teacher))).await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[sqlx::test]
async fn students_authorization_is_preserved(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let other_teacher = {
        let res = send(
            &app,
            json_req(
                "POST",
                "/api/admin/teachers",
                Some(&a.admin),
                Some(json!({"email": "teacher2@example.com"})),
            ),
        )
        .await;
        assert!(res.status().is_success());
        login_as(&app, "sub-teacher2", "teacher2@example.com").await
    };
    let path = format!("/api/classrooms/{}/students?limit=2", a.classroom);
    for (cookie, status) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        // 別人的教室與不存在的教室回應相同
        (Some(other_teacher.as_str()), StatusCode::NOT_FOUND),
        (Some(a.teacher.as_str()), StatusCode::OK),
        (Some(a.admin.as_str()), StatusCode::OK),
    ] {
        assert_eq!(send(&app, get(&path, cookie)).await.status(), status);
    }
}

// ---- GET /api/classrooms/{id}/roster ----

#[sqlx::test]
async fn roster_paginates_by_email_as_objects(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let mut want = enroll(&app, &a, 4).await;
    want.push("student@example.com".into());
    want.sort();
    let base = format!("/api/classrooms/{}/roster?limit=2", a.classroom);
    let pages = all_pages(&app, &base, &a.teacher).await;
    assert_eq!(pages.len(), 3);
    assert_eq!(field(&pages, "email"), want);
    assert_eq!(pages[0]["items"][0], json!({"email": "s01@example.com"}));
    assert_eq!(pages[2]["next_cursor"], Value::Null);
}

#[sqlx::test]
async fn roster_validates_input_and_rejects_foreign_cursors(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    enroll(&app, &a, 3).await;
    let base = format!("/api/classrooms/{}", a.classroom);
    for (query, code) in [
        ("limit=0", "invalid_limit"),
        ("limit=201", "invalid_limit"),
        ("limit=x", "invalid_limit"),
        ("cursor=broken", "invalid_cursor"),
        ("cursor=", "invalid_cursor"),
    ] {
        let (status, got) = error_code(&app, &format!("{base}/roster?{query}"), &a.teacher).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(got, code, "{query}");
    }
    // 學生列表的游標不能用在名單
    let students = json_body(
        send(
            &app,
            get(&format!("{base}/students?limit=1"), Some(&a.teacher)),
        )
        .await,
    )
    .await;
    let cursor = students["next_cursor"].as_str().unwrap();
    let (status, code) =
        error_code(&app, &format!("{base}/roster?cursor={cursor}"), &a.teacher).await;
    assert_eq!(
        (status, code.as_str()),
        (StatusCode::BAD_REQUEST, "invalid_cursor")
    );
}

#[sqlx::test]
async fn roster_empty_page_and_authorization(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let empty = create_classroom(&app, &a.teacher, "空教室").await;
    let page = json_body(
        send(
            &app,
            get(&format!("/api/classrooms/{empty}/roster"), Some(&a.teacher)),
        )
        .await,
    )
    .await;
    assert_eq!(page, json!({"items": [], "next_cursor": null}));
    let path = format!("/api/classrooms/{}/roster", a.classroom);
    for (cookie, status) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        (Some(a.teacher.as_str()), StatusCode::OK),
        (Some(a.admin.as_str()), StatusCode::OK),
    ] {
        assert_eq!(send(&app, get(&path, cookie)).await.status(), status);
    }
}

// ---- GET /api/activities ----

async fn seed_activities(pool: &PgPool, classroom: &str, topic: &str, specs: &[(u128, &str)]) {
    for (n, created) in specs {
        sqlx::query(
            "INSERT INTO activities (id, title, topic_id, classroom_id, status, created_at)
             VALUES ($1, 'Synthetic', $2::uuid, $3::uuid, 'draft', $4::timestamptz)",
        )
        .bind(Uuid::from_u128(*n))
        .bind(topic)
        .bind(classroom)
        .bind(created)
        .execute(pool)
        .await
        .unwrap();
    }
}

#[sqlx::test]
async fn activities_paginate_newest_first_with_stable_ties(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let topic = create_topic(&app, &a, "Synthetic topic").await;
    seed_activities(
        &pool,
        &a.classroom,
        &topic,
        &[
            (6, "2026-01-03T00:00:00Z"),
            (1, "2026-01-02T00:00:00Z"),
            (2, "2026-01-02T00:00:00Z"),
            (3, "2026-01-02T00:00:00Z"),
            (4, "2026-01-02T00:00:00Z"),
            (5, "2026-01-01T00:00:00Z"),
        ],
    )
    .await;
    let pages = all_pages(&app, "/api/activities?limit=2", &a.teacher).await;
    assert_eq!(pages.len(), 3);
    let want: Vec<String> = [6u128, 1, 2, 3, 4, 5]
        .iter()
        .map(|n| Uuid::from_u128(*n).to_string())
        .collect();
    assert_eq!(field(&pages, "id"), want);
    assert_eq!(pages[2]["next_cursor"], Value::Null);
}

#[sqlx::test]
async fn activities_pagination_keeps_classroom_scope(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let topic = create_topic(&app, &a, "Synthetic topic").await;
    let other = create_classroom(&app, &a.teacher, "另一間").await;
    let (t2, room_other) = {
        let res = send(
            &app,
            json_req(
                "POST",
                "/api/admin/teachers",
                Some(&a.admin),
                Some(json!({"email": "teacher2@example.com"})),
            ),
        )
        .await;
        assert!(res.status().is_success());
        let cookie = login_as(&app, "sub-teacher2", "teacher2@example.com").await;
        let room = create_classroom(&app, &cookie, "他班").await;
        (cookie, room)
    };
    seed_activities(
        &pool,
        &a.classroom,
        &topic,
        &[(1, "2026-01-03T00:00:00Z"), (2, "2026-01-02T00:00:00Z")],
    )
    .await;
    seed_activities(&pool, &other, &topic, &[(3, "2026-01-01T00:00:00Z")]).await;
    seed_activities(&pool, &room_other, &topic, &[(4, "2026-01-04T00:00:00Z")]).await;

    let ids =
        |n: &[u128]| -> Vec<String> { n.iter().map(|n| Uuid::from_u128(*n).to_string()).collect() };
    let mine = all_pages(&app, "/api/activities?limit=1", &a.teacher).await;
    assert_eq!(field(&mine, "id"), ids(&[1, 2, 3]));
    let one = all_pages(
        &app,
        &format!("/api/activities?limit=1&classroom_id={}", a.classroom),
        &a.teacher,
    )
    .await;
    assert_eq!(field(&one, "id"), ids(&[1, 2]));
    let theirs = all_pages(&app, "/api/activities?limit=1", &t2).await;
    assert_eq!(field(&theirs, "id"), ids(&[4]));
    let admin = all_pages(&app, "/api/activities?limit=2", &a.admin).await;
    assert_eq!(field(&admin, "id"), ids(&[4, 1, 2, 3]));
    // 別人的教室：404
    let res = send(
        &app,
        get(
            &format!("/api/activities?classroom_id={room_other}"),
            Some(&a.teacher),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn activities_validate_input_default_and_authorization(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let topic = create_topic(&app, &a, "Synthetic topic").await;
    for (query, code) in [
        ("limit=0", "invalid_limit"),
        ("limit=201", "invalid_limit"),
        ("limit=", "invalid_limit"),
        ("cursor=broken", "invalid_cursor"),
        ("cursor=", "invalid_cursor"),
    ] {
        let (status, got) = error_code(&app, &format!("/api/activities?{query}"), &a.teacher).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(got, code, "{query}");
    }
    let empty = json_body(send(&app, get("/api/activities", Some(&a.teacher))).await).await;
    assert_eq!(empty, json!({"items": [], "next_cursor": null}));
    sqlx::query(
        "INSERT INTO activities (title, topic_id, classroom_id, created_at)
         SELECT 'Synthetic', $1::uuid, $2::uuid, now() - make_interval(secs => g)
         FROM generate_series(1, 201) g",
    )
    .bind(&topic)
    .bind(&a.classroom)
    .execute(&pool)
    .await
    .unwrap();
    for (query, count) in [("", 50), ("?limit=200", 200)] {
        let page = json_body(
            send(
                &app,
                get(&format!("/api/activities{query}"), Some(&a.teacher)),
            )
            .await,
        )
        .await;
        assert_eq!(page["items"].as_array().unwrap().len(), count);
        assert!(page["next_cursor"].is_string());
    }
    for (cookie, status) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        (Some(a.teacher.as_str()), StatusCode::OK),
        (Some(a.admin.as_str()), StatusCode::OK),
    ] {
        assert_eq!(
            send(&app, get("/api/activities?limit=2", cookie))
                .await
                .status(),
            status
        );
    }
}
