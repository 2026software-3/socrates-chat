//! F-19.1 點數帳本與防重複入帳（S-07.2）：資料存取層、餘額 API 與授權（合成資料）。

mod common;

use axum::{Router, http::StatusCode};
use common::*;
use serde_json::{Value, json};
use socrates_chat_backend::points::{self, Entry, Source};
use sqlx::PgPool;
use uuid::Uuid;

async fn user_id(pool: &PgPool, email: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// 完成對話發點的帳本紀錄（數值只是測試用）。
fn conversation_entry(user_id: Uuid, conversation_id: &str) -> Entry<'_> {
    Entry {
        user_id,
        delta: 10,
        reason: "conversation_completed",
        source: Source::Conversation,
        source_id: conversation_id,
        unit_price: None,
    }
}

async fn record(pool: &PgPool, entry: &Entry<'_>) -> bool {
    let mut conn = pool.acquire().await.unwrap();
    points::record(&mut conn, entry).await.unwrap()
}

async fn ledger_rows(pool: &PgPool, user_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM point_ledger WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn my_points(app: &Router, cookie: &str) -> Value {
    let res = send(app, get("/api/points", Some(cookie))).await;
    assert_eq!(res.status(), StatusCode::OK);
    json_body(res).await
}

async fn seeded(pool: &PgPool) -> (Router, Actors, Uuid) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let student = user_id(pool, "student@example.com").await;
    (app, a, student)
}

#[sqlx::test]
async fn the_same_event_is_recorded_only_once(pool: PgPool) {
    let (app, a, student) = seeded(&pool).await;
    let conv = Uuid::new_v4().to_string();

    assert!(record(&pool, &conversation_entry(student, &conv)).await);
    // 重複送出：不寫入，也不是錯誤（冪等）
    assert!(!record(&pool, &conversation_entry(student, &conv)).await);

    assert_eq!(ledger_rows(&pool, student).await, 1);
    assert_eq!(my_points(&app, &a.student).await["balance"], 10);
}

#[sqlx::test]
async fn concurrent_duplicates_are_recorded_only_once(pool: PgPool) {
    let (_, _, student) = seeded(&pool).await;
    let conv = Uuid::new_v4().to_string();
    let entry = conversation_entry(student, &conv);

    let (x, y) = tokio::join!(record(&pool, &entry), record(&pool, &entry));

    assert!(x ^ y, "exactly one of the two writes should insert");
    assert_eq!(ledger_rows(&pool, student).await, 1);
}

#[sqlx::test]
async fn different_events_each_get_recorded(pool: PgPool) {
    let (app, a, student) = seeded(&pool).await;
    let (c1, c2) = (Uuid::new_v4().to_string(), Uuid::new_v4().to_string());
    assert!(record(&pool, &conversation_entry(student, &c1)).await);
    assert!(record(&pool, &conversation_entry(student, &c2)).await);
    // 同一個 source_id、不同來源類型是不同事件
    let achievement = Entry {
        source: Source::Achievement,
        reason: "achievement_unlocked",
        ..conversation_entry(student, &c1)
    };
    assert!(record(&pool, &achievement).await);

    assert_eq!(my_points(&app, &a.student).await["balance"], 30);
}

#[sqlx::test]
async fn balance_is_the_sum_of_credits_and_debits(pool: PgPool) {
    let (app, a, student) = seeded(&pool).await;
    let conv = Uuid::new_v4().to_string();
    record(&pool, &conversation_entry(student, &conv)).await;
    let purchase = Entry {
        user_id: student,
        delta: -7,
        reason: "part_unlocked",
        source: Source::Purchase,
        source_id: "part-hat",
        unit_price: Some(7),
    };
    record(&pool, &purchase).await;

    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(points::balance(&mut conn, student).await.unwrap(), 3);
    assert_eq!(my_points(&app, &a.student).await["balance"], 3);
}

#[sqlx::test]
async fn new_student_has_zero_balance(pool: PgPool) {
    let (app, a, _) = seeded(&pool).await;
    let body = my_points(&app, &a.student).await;
    assert_eq!(body, json!({"balance": 0}));
}

#[sqlx::test]
async fn students_see_only_their_balance_not_the_ledger(pool: PgPool) {
    let (app, a, student) = seeded(&pool).await;
    let conv = Uuid::new_v4().to_string();
    record(&pool, &conversation_entry(student, &conv)).await;

    // 只有餘額；不回帳本明細，也沒有對話標題、訊息或總結（S-02.2）
    assert_eq!(my_points(&app, &a.student).await, json!({"balance": 10}));
}

#[sqlx::test]
async fn deleting_a_conversation_keeps_its_points(pool: PgPool) {
    let (app, a, student) = seeded(&pool).await;
    let conv = start_conversation(&app, &a).await;
    record(&pool, &conversation_entry(student, &conv)).await;

    let res = send(
        &app,
        json_req(
            "DELETE",
            &format!("/api/conversations/{conv}"),
            Some(&a.student),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    // 不追回點數，帳本紀錄仍在（S-07.2 2.5）
    assert_eq!(my_points(&app, &a.student).await["balance"], 10);
    assert_eq!(ledger_rows(&pool, student).await, 1);
}

#[sqlx::test]
async fn deleting_the_account_removes_its_ledger(pool: PgPool) {
    let (app, a, student) = seeded(&pool).await;
    record(&pool, &conversation_entry(student, "c1")).await;

    let res = send(
        &app,
        json_req(
            "DELETE",
            "/api/students/student@example.com",
            Some(&a.teacher),
            Some(json!({"confirm_email": "student@example.com"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(ledger_rows(&pool, student).await, 0);
}

#[sqlx::test]
async fn students_only_see_their_own_points(pool: PgPool) {
    let (app, a, student) = seeded(&pool).await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/roster/import",
            Some(&a.admin),
            Some(json!({"text": "other@example.com"})),
        ),
    )
    .await;
    assert!(res.status().is_success());
    let other_cookie = login_as(&app, "sub-other", "other@example.com").await;
    let other = user_id(&pool, "other@example.com").await;
    record(&pool, &conversation_entry(student, "mine")).await;
    record(&pool, &conversation_entry(other, "theirs")).await;
    record(&pool, &conversation_entry(other, "theirs-2")).await;

    assert_eq!(my_points(&app, &a.student).await["balance"], 10);
    assert_eq!(my_points(&app, &other_cookie).await["balance"], 20);
}

#[sqlx::test]
async fn points_are_only_readable_by_enrolled_students(pool: PgPool) {
    let (app, a, student) = seeded(&pool).await;
    record(&pool, &conversation_entry(student, "c1")).await;

    let res = send(&app, get("/api/points", None)).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // 教師（含助教）與管理者都看不到學生點數；名單外帳號尚未開通
    for (cookie, code) in [
        (&a.teacher, "forbidden"),
        (&a.admin, "forbidden"),
        (&a.outsider, "not_enrolled"),
    ] {
        let res = send(&app, get("/api/points", Some(cookie))).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(json_body(res).await["error"]["code"], code);
    }

    // 沒有查詢他人點數的路徑
    let res = send(
        &app,
        get(&format!("/api/points/{student}"), Some(&a.teacher)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn the_database_rejects_unknown_sources_and_negative_prices(pool: PgPool) {
    let (_, _, student) = seeded(&pool).await;
    for (source_type, unit_price) in [("gift", None), ("purchase", Some(-1))] {
        let r = sqlx::query(
            "INSERT INTO point_ledger (user_id, delta, reason, source_type, source_id, unit_price)
             VALUES ($1, -1, 'test', $2, 'x', $3)",
        )
        .bind(student)
        .bind(source_type)
        .bind(unit_price)
        .execute(&pool)
        .await;
        assert!(
            r.is_err(),
            "{source_type} / {unit_price:?} should be rejected"
        );
    }
}

/// 直接寫入資料庫，繞過應用程式，確認約束由資料庫本身保證。
async fn insert_raw(
    pool: &PgPool,
    user_id: Uuid,
    source_type: &str,
    delta: i32,
    unit_price: Option<i32>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO point_ledger (user_id, delta, reason, source_type, source_id, unit_price)
         VALUES ($1, $2, 'test', $3, $4, $5)",
    )
    .bind(user_id)
    .bind(delta)
    .bind(source_type)
    .bind(Uuid::new_v4().to_string())
    .bind(unit_price)
    .execute(pool)
    .await
    .map(|_| ())
}

#[sqlx::test]
async fn the_database_keeps_delta_and_unit_price_consistent(pool: PgPool) {
    let (_, _, student) = seeded(&pool).await;

    // 購買：扣的點數等於當下價格；0 點的免費部件也允許（S-07 §5）
    // 管理者調整可正可負（S-07.2 2.4）
    for (source_type, delta, unit_price) in [
        ("purchase", -30, Some(30)),
        ("purchase", 0, Some(0)),
        ("conversation", 10, None),
        ("task", 5, None),
        ("admin_adjustment", 3, None),
        ("admin_adjustment", -3, None),
    ] {
        assert!(
            insert_raw(&pool, student, source_type, delta, unit_price)
                .await
                .is_ok(),
            "{source_type} / {delta} / {unit_price:?} should be accepted"
        );
    }

    for (source_type, delta, unit_price) in [
        // 購買變成加點、扣點與價格不符、沒有記錄價格
        ("purchase", 30, Some(30)),
        ("purchase", -10, Some(30)),
        ("purchase", -30, None),
        // 沒有變動的紀錄會佔掉 UNIQUE 位置，之後無法補發
        ("conversation", 0, None),
        ("admin_adjustment", 0, None),
        // 只有扣除記錄價格
        ("conversation", 10, Some(10)),
        ("admin_adjustment", -5, Some(5)),
    ] {
        assert!(
            insert_raw(&pool, student, source_type, delta, unit_price)
                .await
                .is_err(),
            "{source_type} / {delta} / {unit_price:?} should be rejected"
        );
    }
}

#[sqlx::test]
async fn migrations_can_run_again_on_a_migrated_database(pool: PgPool) {
    sqlx::migrate!().run(&pool).await.unwrap();
    sqlx::migrate!().run(&pool).await.unwrap();
}
