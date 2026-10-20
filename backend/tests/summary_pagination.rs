//! `GET /api/summaries` 的分頁、篩選與權限（#285）。資料皆為合成資料。
mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

async fn user_id(pool: &PgPool, email: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// 直接寫入一場已結束、總結已完成的對話。
async fn seed_ready(pool: &PgPool, id: Uuid, user: Uuid, title: &str, ended_at: &str) {
    sqlx::query(
        "INSERT INTO conversations (id, user_id, title, status, ended_at)
         VALUES ($1, $2, $3, 'ended', $4::timestamptz)",
    )
    .bind(id)
    .bind(user)
    .bind(title)
    .bind(ended_at)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO summaries (conversation_id, status, stance, reasons, claim, framework)
         VALUES ($1, 'ready', 'Synthetic stance', 'Synthetic reasons', 'Synthetic claim', 'care')",
    )
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
}

/// 依 next_cursor 走完所有頁，回傳每頁內容。
async fn all_pages(app: &axum::Router, base: &str, cookie: &str) -> Vec<Value> {
    let sep = if base.contains('?') { '&' } else { '?' };
    let mut path = base.to_string();
    let mut pages = Vec::new();
    loop {
        let res = send(app, get(&path, Some(cookie))).await;
        assert_eq!(res.status(), StatusCode::OK, "{path}");
        let page = json_body(res).await;
        let next = page["next_cursor"].as_str().map(str::to_string);
        pages.push(page);
        assert!(pages.len() <= 20, "too many pages");
        match next {
            Some(cursor) => path = format!("{base}{sep}cursor={cursor}"),
            None => break,
        }
    }
    pages
}

fn ids(pages: &[Value]) -> Vec<String> {
    pages
        .iter()
        .flat_map(|p| p["items"].as_array().unwrap().iter())
        .map(|r| r["conversation_id"].as_str().unwrap().to_string())
        .collect()
}

/// 兩位學生、兩個題目；ended_at 有相同的（測試 tie-breaker）。依 ended_at DESC, conversation_id ASC 排序。
async fn seed_many(pool: &PgPool) -> (Uuid, Uuid, Vec<(Uuid, Uuid, &'static str)>) {
    sqlx::query(
        "INSERT INTO users (email, display_name) VALUES ('student2@example.com', 'Synthetic two')",
    )
    .execute(pool)
    .await
    .unwrap();
    let s1 = user_id(pool, "student@example.com").await;
    let s2 = user_id(pool, "student2@example.com").await;
    // (id, 學生, 題目, 結束時間)，已依預期順序排好
    let rows = [
        (7u128, s2, "Topic B", "2026-01-03T00:00:00Z"),
        (1, s1, "Topic A", "2026-01-02T00:00:00Z"),
        (2, s2, "Topic A", "2026-01-02T00:00:00Z"),
        (3, s1, "Topic B", "2026-01-02T00:00:00Z"),
        (4, s1, "Topic A", "2026-01-02T00:00:00Z"),
        (5, s2, "Topic B", "2026-01-02T00:00:00Z"),
        (6, s1, "Topic A", "2026-01-01T00:00:00Z"),
    ];
    let mut expected = Vec::new();
    for (n, user, title, ended) in rows {
        let id = Uuid::from_u128(n);
        seed_ready(pool, id, user, title, ended).await;
        expected.push((id, user, title));
    }
    (s1, s2, expected)
}

#[sqlx::test]
async fn paginates_in_stable_order_without_duplicates_or_gaps(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (_, _, expected) = seed_many(&pool).await;
    // 未完成的總結不出現
    let user = user_id(&pool, "student@example.com").await;
    sqlx::query("INSERT INTO conversations (id, user_id, title, status, ended_at) VALUES ($1, $2, 'Pending', 'ended', now())")
        .bind(Uuid::from_u128(99)).bind(user).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO summaries (conversation_id) VALUES ($1)")
        .bind(Uuid::from_u128(99))
        .execute(&pool)
        .await
        .unwrap();

    let pages = all_pages(&app, "/api/summaries?limit=2", &a.teacher).await;
    assert_eq!(pages.len(), 4);
    for p in &pages[..3] {
        assert_eq!(p["items"].as_array().unwrap().len(), 2);
        assert!(p["next_cursor"].is_string());
    }
    assert_eq!(pages[3]["next_cursor"], Value::Null);
    let want: Vec<String> = expected.iter().map(|e| e.0.to_string()).collect();
    assert_eq!(ids(&pages), want);
    // 教師看得到總結內容
    assert_eq!(pages[0]["items"][0]["stance"], "Synthetic stance");
}

#[sqlx::test]
async fn admin_sees_masked_fields_on_every_page(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    seed_many(&pool).await;
    let pages = all_pages(&app, "/api/summaries?limit=3", &a.admin).await;
    assert_eq!(pages.len(), 3);
    assert_eq!(ids(&pages).len(), 7);
    for p in &pages {
        for row in p["items"].as_array().unwrap() {
            for f in ["stance", "reasons", "claim", "framework"] {
                assert_eq!(row[f], "message", "{f}");
            }
            assert_eq!(row["framework_scores"], Value::Null);
        }
        assert!(!p.to_string().contains("Synthetic stance"));
        assert!(!p.to_string().contains("Synthetic claim"));
    }
}

#[sqlx::test]
async fn filters_combine_with_pagination(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (s1, s2, expected) = seed_many(&pool).await;
    let pick = |f: &dyn Fn(&(Uuid, Uuid, &str)) -> bool| -> Vec<String> {
        expected
            .iter()
            .filter(|e| f(e))
            .map(|e| e.0.to_string())
            .collect()
    };
    for (query, want) in [
        (format!("student={s1}"), pick(&|e| e.1 == s1)),
        (format!("student={s2}"), pick(&|e| e.1 == s2)),
        ("topic=Topic%20A".to_string(), pick(&|e| e.2 == "Topic A")),
        (
            format!("student={s1}&topic=Topic%20A"),
            pick(&|e| e.1 == s1 && e.2 == "Topic A"),
        ),
        (
            format!("student={s2}&topic=Topic%20B"),
            pick(&|e| e.1 == s2 && e.2 == "Topic B"),
        ),
    ] {
        let pages = all_pages(&app, &format!("/api/summaries?limit=1&{query}"), &a.teacher).await;
        assert_eq!(ids(&pages), want, "{query}");
    }
    // 不存在的學生或題目：空結果
    for query in [
        format!("student={}", Uuid::from_u128(12345)),
        "topic=No%20such%20topic".to_string(),
        "topic=topic%20a".to_string(),
    ] {
        let page = json_body(
            send(
                &app,
                get(&format!("/api/summaries?{query}"), Some(&a.teacher)),
            )
            .await,
        )
        .await;
        assert_eq!(page, json!({"items": [], "next_cursor": null}), "{query}");
    }
}

#[sqlx::test]
async fn validates_input(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    for (query, code) in [
        ("limit=0", "invalid_limit"),
        ("limit=201", "invalid_limit"),
        ("limit=-1", "invalid_limit"),
        ("limit=x", "invalid_limit"),
        ("limit=", "invalid_limit"),
        ("cursor=broken", "invalid_cursor"),
        ("cursor=", "invalid_cursor"),
        ("cursor=e30", "invalid_cursor"),
        ("student=not-a-uuid", "invalid_filter"),
        ("student=", "invalid_filter"),
        ("topic=", "invalid_filter"),
        ("topic=%20%20", "invalid_filter"),
    ] {
        let res = send(
            &app,
            get(&format!("/api/summaries?{query}"), Some(&a.teacher)),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(json_body(res).await["error"]["code"], code, "{query}");
    }
    // 對話列表的游標不能拿來用
    let user = user_id(&pool, "student@example.com").await;
    sqlx::query(
        "INSERT INTO conversations (user_id,title) SELECT $1,'Synthetic' FROM generate_series(1,2)",
    )
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    let convs =
        json_body(send(&app, get("/api/conversations?limit=1", Some(&a.student))).await).await;
    let foreign = convs["next_cursor"].as_str().unwrap();
    let res = send(
        &app,
        get(
            &format!("/api/summaries?cursor={foreign}"),
            Some(&a.teacher),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(res).await["error"]["code"], "invalid_cursor");

    let empty = json_body(send(&app, get("/api/summaries", Some(&a.teacher))).await).await;
    assert_eq!(empty, json!({"items": [], "next_cursor": null}));
}

#[sqlx::test]
async fn default_and_maximum_page_sizes(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let user = user_id(&pool, "student@example.com").await;
    sqlx::query(
        "WITH c AS (
             INSERT INTO conversations (user_id, title, status, ended_at)
             SELECT $1, 'Synthetic', 'ended', now() - make_interval(secs => g)
             FROM generate_series(1, 201) g RETURNING id)
         INSERT INTO summaries (conversation_id, status, stance, reasons)
         SELECT id, 'ready', 's', 'r' FROM c",
    )
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    for (query, count) in [("", 50), ("?limit=200", 200)] {
        let page = json_body(
            send(
                &app,
                get(&format!("/api/summaries{query}"), Some(&a.teacher)),
            )
            .await,
        )
        .await;
        assert_eq!(page["items"].as_array().unwrap().len(), count);
        assert!(page["next_cursor"].is_string());
    }
}

#[sqlx::test]
async fn only_teachers_and_admins_may_list(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    for (cookie, status) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        (Some(a.teacher.as_str()), StatusCode::OK),
        (Some(a.admin.as_str()), StatusCode::OK),
    ] {
        assert_eq!(
            send(&app, get("/api/summaries?limit=2", cookie))
                .await
                .status(),
            status
        );
    }
}

#[sqlx::test]
async fn old_teacher_path_is_gone(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let res = send(&app, get("/api/teacher/summaries", Some(&a.teacher))).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
