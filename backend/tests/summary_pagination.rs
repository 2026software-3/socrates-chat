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

/// 直接寫入一場已結束、總結已完成的對話；總結的完成時間與結束時間相同。
async fn seed_ready(
    pool: &PgPool,
    classroom: Uuid,
    id: Uuid,
    user: Uuid,
    title: &str,
    ended_at: &str,
) {
    sqlx::query(
        "INSERT INTO conversations (id, user_id, classroom_id, title, status, ended_at)
         VALUES ($1, $2, $3, $4, 'ended', $5::timestamptz)",
    )
    .bind(id)
    .bind(user)
    .bind(classroom)
    .bind(title)
    .bind(ended_at)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO summaries (conversation_id, status, stance, reasons, claim, completed_at)
         VALUES ($1, 'ready', 'Synthetic stance', 'Synthetic reasons', 'Synthetic claim', $2::timestamptz)",
    )
    .bind(id)
    .bind(ended_at)
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

/// 兩位學生、兩個題目；完成時間有相同的（測試 tie-breaker）。依 completed_at DESC, conversation_id ASC 排序。
async fn seed_many(
    pool: &PgPool,
    classroom: &str,
) -> (Uuid, Uuid, Vec<(Uuid, Uuid, &'static str)>) {
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
        seed_ready(pool, classroom.parse().unwrap(), id, user, title, ended).await;
        expected.push((id, user, title));
    }
    (s1, s2, expected)
}

#[sqlx::test]
async fn paginates_in_stable_order_without_duplicates_or_gaps(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (_, _, expected) = seed_many(&pool, &a.classroom).await;
    // 未完成的總結不出現
    let user = user_id(&pool, "student@example.com").await;
    sqlx::query("INSERT INTO conversations (id, user_id, classroom_id, title, status, ended_at) VALUES ($1, $2, $3, 'Pending', 'ended', now())")
        .bind(Uuid::from_u128(99)).bind(user).bind(a.classroom.parse::<Uuid>().unwrap()).execute(&pool).await.unwrap();
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
async fn summary_ready_between_pages_appears_on_the_first_page_without_shifting_others(
    pool: PgPool,
) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (s1, _, expected) = seed_many(&pool, &a.classroom).await;
    let first = json_body(send(&app, get("/api/summaries?limit=3", Some(&a.teacher))).await).await;
    let cursor = first["next_cursor"].as_str().unwrap().to_string();

    // 翻頁之間，一場最早結束的對話這時才產生總結（例如重試成功）
    let late = Uuid::from_u128(8);
    sqlx::query(
        "INSERT INTO conversations (id, user_id, classroom_id, title, status, ended_at)
         VALUES ($1, $2, $3, 'Topic A', 'ended', '2025-12-31T00:00:00Z')",
    )
    .bind(late)
    .bind(s1)
    .bind(a.classroom.parse::<Uuid>().unwrap())
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO summaries (conversation_id, status, stance, reasons, completed_at)
         VALUES ($1, 'ready', 'Synthetic stance', 'Synthetic reasons', now())",
    )
    .bind(late)
    .execute(&pool)
    .await
    .unwrap();

    // 往後翻：原有的總結不重複、不遺漏，順序不受影響
    let mut pages = vec![first];
    let mut next = Some(cursor);
    while let Some(cursor) = next {
        let path = format!("/api/summaries?limit=3&cursor={cursor}");
        let page = json_body(send(&app, get(&path, Some(&a.teacher))).await).await;
        next = page["next_cursor"].as_str().map(str::to_string);
        pages.push(page);
    }
    let seen = ids(&pages);
    let want: Vec<String> = expected.iter().map(|e| e.0.to_string()).collect();
    assert_eq!(seen, want);
    // 重新開啟列表：新完成的總結在第一頁最上面
    let reopened =
        json_body(send(&app, get("/api/summaries?limit=3", Some(&a.teacher))).await).await;
    assert_eq!(reopened["items"][0]["conversation_id"], late.to_string());
}

#[sqlx::test]
async fn admin_sees_masked_fields_on_every_page(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    seed_many(&pool, &a.classroom).await;
    let pages = all_pages(&app, "/api/summaries?limit=3", &a.admin).await;
    assert_eq!(pages.len(), 3);
    assert_eq!(ids(&pages).len(), 7);
    for p in &pages {
        for row in p["items"].as_array().unwrap() {
            for f in ["stance", "reasons", "claim"] {
                assert_eq!(row[f], "message", "{f}");
            }
        }
        assert!(!p.to_string().contains("Synthetic stance"));
        assert!(!p.to_string().contains("Synthetic claim"));
    }
}

#[sqlx::test]
async fn filters_combine_with_pagination(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (s1, s2, expected) = seed_many(&pool, &a.classroom).await;
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
        // 題目標題不分大小寫
        ("topic=tOPIC%20a".to_string(), pick(&|e| e.2 == "Topic A")),
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
        // 完全相符：子字串不算
        "topic=Topic".to_string(),
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
async fn q_searches_name_email_and_title_across_all_pages(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (s1, s2, expected) = seed_many(&pool, &a.classroom).await;
    let pick = |f: &dyn Fn(&(Uuid, Uuid, &str)) -> bool| -> Vec<String> {
        expected
            .iter()
            .filter(|e| f(e))
            .map(|e| e.0.to_string())
            .collect()
    };
    for (query, want) in [
        // 電子郵件，不分大小寫
        ("q=STUDENT2%40EXAMPLE", pick(&|e| e.1 == s2)),
        // 顯示名稱（Synthetic two），子字串
        ("q=thetic%20TWO", pick(&|e| e.1 == s2)),
        // 題目標題，不分大小寫
        ("q=topic%20a", pick(&|e| e.2 == "Topic A")),
        // 與 student 篩選並用
        (
            "q=topic%20b&student=SELF",
            pick(&|e| e.1 == s1 && e.2 == "Topic B"),
        ),
        // 兩邊都不符
        ("q=no-such-text", vec![]),
        // LIKE 萬用字元要當成一般字元
        ("q=%25", vec![]),
        ("q=_", vec![]),
    ] {
        let query = query.replace("SELF", &s1.to_string());
        // limit=1 逐頁走完：搜尋要涵蓋所有頁，不只是第一頁
        let pages = all_pages(&app, &format!("/api/summaries?limit=1&{query}"), &a.teacher).await;
        assert_eq!(ids(&pages), want, "{query}");
    }
}

#[sqlx::test]
async fn cursor_only_works_with_the_filters_that_produced_it(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (s1, _, _) = seed_many(&pool, &a.classroom).await;
    let next = |query: String| {
        let (app, cookie) = (app.clone(), a.teacher.clone());
        async move {
            let page = json_body(
                send(
                    &app,
                    get(&format!("/api/summaries?limit=1&{query}"), Some(&cookie)),
                )
                .await,
            )
            .await;
            page["next_cursor"].as_str().unwrap().to_string()
        }
    };
    let status_with = |query: String, cursor: String| {
        let (app, cookie) = (app.clone(), a.teacher.clone());
        async move {
            let res = send(
                &app,
                get(
                    &format!("/api/summaries?limit=1&{query}&cursor={cursor}"),
                    Some(&cookie),
                ),
            )
            .await;
            let status = res.status();
            (status, json_body(res).await)
        }
    };
    let topic_a = next("topic=Topic%20A".to_string()).await;
    // 條件不同（換題目、拿掉、加上其他條件）：拒絕
    for query in [
        "topic=Topic%20B".to_string(),
        "x=1".to_string(),
        format!("topic=Topic%20A&student={s1}"),
        "topic=Topic%20A&q=topic".to_string(),
        "q=topic".to_string(),
    ] {
        let (status, body) = status_with(query.clone(), topic_a.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(body["error"]["code"], "invalid_cursor", "{query}");
    }
    // 沒有條件的游標不能套到有條件的請求
    let unfiltered = next("x=1".to_string()).await;
    let (status, _) = status_with("topic=Topic%20A".to_string(), unfiltered.clone()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // 條件相同（題目不分大小寫、q 前後空白與大小寫不影響）：照常使用
    let (status, _) = status_with("topic=tOPIC%20a".to_string(), topic_a).await;
    assert_eq!(status, StatusCode::OK);
    let q = next("q=%20STUDENT2%40example%20".to_string()).await;
    let (status, _) = status_with("q=student2%40EXAMPLE".to_string(), q.clone()).await;
    assert_eq!(status, StatusCode::OK);
    // 游標不帶出搜尋字串（姓名、電子郵件）
    let decoded = String::from_utf8(
        base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &q).unwrap(),
    )
    .unwrap();
    assert!(!decoded.to_lowercase().contains("student2"), "{decoded}");
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
        ("q=", "invalid_filter"),
        ("q=%20%20", "invalid_filter"),
    ] {
        let res = send(
            &app,
            get(&format!("/api/summaries?{query}"), Some(&a.teacher)),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(json_body(res).await["error"]["code"], code, "{query}");
    }
    let too_long = "a".repeat(101);
    let res = send(
        &app,
        get(&format!("/api/summaries?q={too_long}"), Some(&a.teacher)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(res).await["error"]["code"], "invalid_filter");
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
             INSERT INTO conversations (user_id, classroom_id, title, status, ended_at)
             SELECT $1, $2, 'Synthetic', 'ended', now() - make_interval(secs => g)
             FROM generate_series(1, 201) g RETURNING id)
         INSERT INTO summaries (conversation_id, status, stance, reasons, completed_at)
         SELECT id, 'ready', 's', 'r', now() FROM c",
    )
    .bind(user)
    .bind(a.classroom.parse::<Uuid>().unwrap())
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
async fn classroom_filter_applies_and_binds_the_cursor(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    seed_many(&pool, &a.classroom).await;
    let other = create_classroom(&app, &a.teacher, "另一間").await;
    let user = user_id(&pool, "student@example.com").await;
    seed_ready(
        &pool,
        other.parse().unwrap(),
        Uuid::from_u128(50),
        user,
        "Topic C",
        "2026-02-01T00:00:00Z",
    )
    .await;

    let pages = all_pages(
        &app,
        &format!("/api/summaries?limit=2&classroom_id={other}"),
        &a.teacher,
    )
    .await;
    assert_eq!(ids(&pages), [Uuid::from_u128(50).to_string()]);
    let pages = all_pages(
        &app,
        &format!("/api/summaries?limit=3&classroom_id={}", a.classroom),
        &a.teacher,
    )
    .await;
    assert_eq!(ids(&pages).len(), 7);
    // 不分教室的游標不能套到某間教室
    let first = json_body(send(&app, get("/api/summaries?limit=1", Some(&a.teacher))).await).await;
    let cursor = first["next_cursor"].as_str().unwrap();
    let res = send(
        &app,
        get(
            &format!("/api/summaries?limit=1&classroom_id={other}&cursor={cursor}"),
            Some(&a.teacher),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(res).await["error"]["code"], "invalid_cursor");
}

#[sqlx::test]
async fn activity_filter_limits_to_one_activity(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    seed_many(&pool, &a.classroom).await;
    let topic = create_topic(&app, &a, "Filter topic").await;
    let activity = create_activity(&app, &a, "Activity one", &topic).await;
    sqlx::query("UPDATE conversations SET activity_id = $1 WHERE id = ANY($2)")
        .bind(activity.parse::<Uuid>().unwrap())
        .bind(vec![Uuid::from_u128(1), Uuid::from_u128(2)])
        .execute(&pool)
        .await
        .unwrap();

    let pages = all_pages(
        &app,
        &format!("/api/summaries?limit=1&activity={activity}"),
        &a.teacher,
    )
    .await;
    assert_eq!(
        ids(&pages),
        vec![
            Uuid::from_u128(1).to_string(),
            Uuid::from_u128(2).to_string()
        ]
    );

    for bad in ["not-a-uuid".to_string(), Uuid::from_u128(777).to_string()] {
        let res = send(
            &app,
            get(&format!("/api/summaries?activity={bad}"), Some(&a.teacher)),
        )
        .await;
        if bad == "not-a-uuid" {
            assert_json_error(res, StatusCode::BAD_REQUEST, "invalid_filter").await;
        } else {
            assert_eq!(
                json_body(res).await,
                json!({"items": [], "next_cursor": null})
            );
        }
    }
}
