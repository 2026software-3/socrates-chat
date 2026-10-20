//! 學生、修課名單、活動列表的分頁（#286–#288），沿用對話列表的分頁慣例（合成資料）。
mod common;
use axum::{Router, http::StatusCode};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// 以 limit=2 一路跟著 next_cursor 取完，回傳每筆的 `field`。
async fn collect(app: &Router, cookie: &str, base: &str, field: &str) -> Vec<String> {
    let mut path = format!("{base}?limit=2");
    let mut got = Vec::new();
    loop {
        let res = send(app, get(&path, Some(cookie))).await;
        assert_eq!(res.status(), StatusCode::OK, "{path}");
        let page = json_body(res).await;
        let items = page["items"].as_array().expect("page items");
        assert!(!items.is_empty() && items.len() <= 2);
        for item in items {
            got.push(item[field].as_str().unwrap().to_string());
        }
        match page["next_cursor"].as_str() {
            Some(cursor) => path = format!("{base}?limit=2&cursor={cursor}"),
            None => break,
        }
        assert!(got.len() <= 20);
    }
    got
}

/// limit／cursor 不合法一律 400；另一種資源的游標也不能拿來用。
async fn assert_rejects_bad_input(app: &Router, cookie: &str, base: &str) {
    let foreign = {
        // 對話列表的游標（resource 不同）
        let bytes = serde_json::to_vec(&(1u8, "conversations", ("x", 1))).unwrap();
        URL_SAFE_NO_PAD.encode(bytes)
    };
    for (query, code) in [
        ("limit=0".to_string(), "invalid_limit"),
        ("limit=201".to_string(), "invalid_limit"),
        ("limit=-1".to_string(), "invalid_limit"),
        ("limit=x".to_string(), "invalid_limit"),
        ("limit=".to_string(), "invalid_limit"),
        ("cursor=broken".to_string(), "invalid_cursor"),
        ("cursor=".to_string(), "invalid_cursor"),
        (format!("cursor={foreign}"), "invalid_cursor"),
    ] {
        let res = send(app, get(&format!("{base}?{query}"), Some(cookie))).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{base}?{query}");
        assert_eq!(
            json_body(res).await["error"]["code"],
            code,
            "{base}?{query}"
        );
    }
}

async fn assert_page_sizes(app: &Router, cookie: &str, base: &str) {
    for (query, count) in [("", 50), ("?limit=200", 200)] {
        let page: Value =
            json_body(send(app, get(&format!("{base}{query}"), Some(cookie))).await).await;
        assert_eq!(
            page["items"].as_array().unwrap().len(),
            count,
            "{base}{query}"
        );
        assert!(page["next_cursor"].is_string());
    }
}

const EMPTY: fn() -> Value = || json!({"items": [], "next_cursor": null});

#[sqlx::test]
async fn students_and_roster_paginate_by_email(pool: PgPool) {
    let app = test_app(pool.clone());
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    // 名單為空
    for base in ["/api/students", "/api/roster"] {
        let page = json_body(send(&app, get(base, Some(&admin))).await).await;
        assert_eq!(page, EMPTY(), "{base}");
    }

    let a = seed_actors(&app).await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/roster/import",
            Some(&a.teacher),
            Some(json!({"text": "e@x.com\nb@x.com\nd@x.com\na@x.com\nc@x.com"})),
        ),
    )
    .await;
    assert!(res.status().is_success());
    let expected = [
        "a@x.com",
        "b@x.com",
        "c@x.com",
        "d@x.com",
        "e@x.com",
        "student@example.com",
    ];
    for cookie in [&a.teacher, &a.admin] {
        for base in ["/api/students", "/api/roster"] {
            assert_eq!(
                collect(&app, cookie, base, "email").await,
                expected,
                "{base}"
            );
            assert_rejects_bad_input(&app, cookie, base).await;
        }
    }
    // 學生列表只有帳號狀態與完成數量
    let page = json_body(send(&app, get("/api/students?limit=1", Some(&a.teacher))).await).await;
    let keys: Vec<&String> = page["items"][0].as_object().unwrap().keys().collect();
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

    // 名單學生的游標不能拿去用在修課名單（反之亦然）
    let student_cursor = json_body(
        send(&app, get("/api/students?limit=1", Some(&a.teacher))).await,
    )
    .await["next_cursor"]
        .as_str()
        .unwrap()
        .to_string();
    let res = send(
        &app,
        get(
            &format!("/api/roster?cursor={student_cursor}"),
            Some(&a.teacher),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    for (cookie, status) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
    ] {
        for base in ["/api/students?limit=2", "/api/roster?limit=2"] {
            assert_eq!(
                send(&app, get(base, cookie)).await.status(),
                status,
                "{base}"
            );
        }
    }

    sqlx::query("INSERT INTO enrollments (email) SELECT 'bulk' || n || '@x.com' FROM generate_series(1,201) n")
        .execute(&pool)
        .await
        .unwrap();
    for base in ["/api/students", "/api/roster"] {
        assert_page_sizes(&app, &a.teacher, base).await;
    }
}

#[sqlx::test]
async fn activities_paginate_newest_first_with_stable_ties(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let page = json_body(send(&app, get("/api/activities", Some(&a.teacher))).await).await;
    assert_eq!(page, EMPTY());

    // 同一時間建立的活動依 id 排序；草稿、已發布、已停止開放都列出
    let mut expected = Vec::new();
    for (i, status) in ["draft", "published", "closed", "draft", "published"]
        .iter()
        .enumerate()
    {
        let id = Uuid::from_u128(i as u128 + 1);
        sqlx::query("INSERT INTO activities (id,title,status,created_at) VALUES ($1,'Synthetic',$2,'2026-01-01T00:00:00Z')")
            .bind(id)
            .bind(status)
            .execute(&pool)
            .await
            .unwrap();
        expected.push(id.to_string());
    }
    let newest = Uuid::from_u128(99);
    sqlx::query("INSERT INTO activities (id,title,created_at) VALUES ($1,'Newer synthetic','2026-01-02T00:00:00Z')")
        .bind(newest)
        .execute(&pool)
        .await
        .unwrap();
    expected.insert(0, newest.to_string());

    for cookie in [&a.teacher, &a.admin] {
        assert_eq!(
            collect(&app, cookie, "/api/activities", "id").await,
            expected
        );
        assert_rejects_bad_input(&app, cookie, "/api/activities").await;
    }
    for (cookie, status) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            send(&app, get("/api/activities?limit=2", cookie))
                .await
                .status(),
            status
        );
    }

    sqlx::query("INSERT INTO activities (title) SELECT 'Synthetic' FROM generate_series(1,201)")
        .execute(&pool)
        .await
        .unwrap();
    assert_page_sizes(&app, &a.teacher, "/api/activities").await;
}
