mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test]
async fn paginates_with_stable_ties_and_excludes_other_students(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let user: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = 'student@example.com'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let mut expected = Vec::new();
    for i in 0..5u128 {
        let id = Uuid::from_u128(i + 1);
        sqlx::query("INSERT INTO conversations (id,user_id,title,created_at) VALUES ($1,$2,'Synthetic','2026-01-01T00:00:00Z')").bind(id).bind(user).execute(&pool).await.unwrap();
        expected.push(id.to_string());
    }
    let newest = Uuid::from_u128(6);
    sqlx::query("INSERT INTO conversations (id,user_id,title,created_at) VALUES ($1,$2,'Newer synthetic','2026-01-02T00:00:00Z')").bind(newest).bind(user).execute(&pool).await.unwrap();
    expected.insert(0, newest.to_string());
    sqlx::query("INSERT INTO conversations (user_id,title) SELECT id,'Other synthetic' FROM users WHERE email = 'outsider@example.com'").execute(&pool).await.unwrap();
    let mut path = "/api/conversations?limit=2".to_string();
    let mut got = Vec::new();
    loop {
        let res = send(&app, get(&path, Some(&a.student))).await;
        assert_eq!(res.status(), StatusCode::OK);
        let page = json_body(res).await;
        let items = page["items"].as_array().expect("page items");
        assert!(!items.is_empty() && items.len() <= 2);
        for item in items {
            assert!(item.get("messages").is_none());
            got.push(item["id"].as_str().unwrap().to_string());
        }
        match page["next_cursor"].as_str() {
            Some(cursor) => path = format!("/api/conversations?limit=2&cursor={cursor}"),
            None => break,
        }
        assert!(got.len() <= 6);
    }
    assert_eq!(got, expected);
}

#[sqlx::test]
async fn pagination_validates_input_and_preserves_authorization(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    for (query, code) in [
        ("limit=0", "invalid_limit"),
        ("limit=201", "invalid_limit"),
        ("limit=-1", "invalid_limit"),
        ("limit=x", "invalid_limit"),
        ("limit=1.5", "invalid_limit"),
        ("limit=", "invalid_limit"),
        ("limit=9999999999999999999999999", "invalid_limit"),
        ("cursor=broken", "invalid_cursor"),
        ("cursor=", "invalid_cursor"),
        ("cursor=e30", "invalid_cursor"),
    ] {
        let res = send(
            &app,
            get(&format!("/api/conversations?{query}"), Some(&a.student)),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(json_body(res).await["error"]["code"], code);
    }
    let empty = json_body(send(&app, get("/api/conversations", Some(&a.student))).await).await;
    assert_eq!(empty, json!({"items":[],"next_cursor":null}));
    for (cookie, status) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(a.teacher.as_str()), StatusCode::FORBIDDEN),
        (Some(a.admin.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            send(&app, get("/api/conversations?limit=2", cookie))
                .await
                .status(),
            status
        );
    }
    sqlx::query("INSERT INTO conversations (user_id,title) SELECT u.id,'Synthetic' FROM users u CROSS JOIN generate_series(1,201) WHERE email='student@example.com'").execute(&pool).await.unwrap();
    for (query, count) in [("", 50), ("?limit=200", 200)] {
        let page: Value = json_body(
            send(
                &app,
                get(&format!("/api/conversations{query}"), Some(&a.student)),
            )
            .await,
        )
        .await;
        assert_eq!(page["items"].as_array().unwrap().len(), count);
        assert!(page["next_cursor"].is_string());
    }
}
