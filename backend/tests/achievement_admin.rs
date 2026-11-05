//! F-18.1 / S-07.3 第 5 節：管理者維護成就、稽核日誌與追溯。全部使用合成資料。

mod common;

use axum::http::StatusCode;
use chrono::{Duration, TimeZone, Utc};
use common::*;
use serde_json::{Value, json};
use socrates_chat_backend::{
    achievements,
    points::{self, Entry, Source},
};
use sqlx::PgPool;
use uuid::Uuid;

const LIST: &str = "/api/admin/achievements";

fn new_achievement(id: &str, kind: &str, threshold: i64) -> Value {
    json!({
        "id": id,
        "condition_type": kind,
        "threshold": threshold,
        "name": {"zh_tw": "新成就", "en": "New", "es": "Nuevo"},
        "description": {"zh_tw": "說明", "en": "Description", "es": "Descripción"},
    })
}

async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    cookie: &str,
    body: Value,
) -> (StatusCode, Value) {
    let res = send(app, json_req(method, path, Some(cookie), Some(body))).await;
    let status = res.status();
    (status, json_body(res).await)
}

async fn student(pool: &PgPool, n: usize) -> Uuid {
    seed_user(pool, &format!("achievement-{n}@example.com")).await
}

/// 讓學生在連續的 Asia/Taipei 日子各完成一場，共 `count` 場（同一天）。
async fn complete_n(pool: &PgPool, user_id: Uuid, count: usize) {
    let at = Utc.with_ymd_and_hms(2026, 10, 24, 4, 0, 0).unwrap();
    for _ in 0..count {
        achievements::record_completed_conversation(pool, user_id, Uuid::new_v4(), at)
            .await
            .unwrap();
    }
}

async fn unlocked(pool: &PgPool, user_id: Uuid, id: &str, level: i32) -> bool {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM achievement_unlocks
         WHERE user_id = $1 AND achievement_id = $2 AND level = $3)",
    )
    .bind(user_id)
    .bind(id)
    .bind(level)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn ledger_rows(pool: &PgPool, user_id: Uuid, id: &str) -> Vec<(i32, String)> {
    sqlx::query_as(
        "SELECT delta, source_id FROM point_ledger
         WHERE user_id = $1 AND source_type = 'achievement' AND source_id LIKE $2 ORDER BY source_id",
    )
    .bind(user_id)
    .bind(format!("{id}:%"))
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn all_ledger_rows(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM point_ledger")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn only_admins_can_read_or_change_achievements(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let body = new_achievement("denied", "completed_conversations", 2);

    for cookie in [&a.teacher, &a.student, &a.outsider] {
        for (method, path, b) in [
            ("GET", LIST.to_string(), Value::Null),
            ("POST", LIST.to_string(), body.clone()),
            (
                "PATCH",
                format!("{LIST}/first_dialogue"),
                json!({"enabled": false}),
            ),
        ] {
            let res = send(
                &app,
                json_req(
                    method,
                    &path,
                    Some(cookie),
                    (!b.is_null()).then(|| b.clone()),
                ),
            )
            .await;
            assert_eq!(res.status(), StatusCode::FORBIDDEN, "{method} {path}");
        }
    }
    for (method, path, b) in [
        ("GET", LIST.to_string(), None),
        ("POST", LIST.to_string(), Some(body.clone())),
        (
            "PATCH",
            format!("{LIST}/first_dialogue"),
            Some(json!({"enabled": false})),
        ),
    ] {
        let res = send(&app, json_req(method, &path, None, b)).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{method} {path}");
    }

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM achievements WHERE id = 'denied'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let enabled: bool = sqlx::query_scalar(
        "SELECT bool_and(enabled) FROM achievements WHERE id = 'first_dialogue'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let audits: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action LIKE 'achievement_%'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((count, enabled, audits), (0, true, 0));
}

#[sqlx::test]
async fn admin_lists_the_seeded_catalog_with_levels_and_three_languages(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;

    let res = send(&app, get(LIST, Some(&a.admin))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let list = json_body(res).await;
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 6, "six seeded achievements, one row each");
    let path = list.iter().find(|x| x["id"] == "dialogue_path").unwrap();
    assert_eq!(path["condition_type"], "completed_conversations");
    assert_eq!(path["enabled"], true);
    assert_eq!(
        path["levels"],
        json!([
            {"level": 1, "threshold": 5, "points": 10},
            {"level": 2, "threshold": 20, "points": 10},
            {"level": 3, "threshold": 50, "points": 10},
        ])
    );
    assert_eq!(path["name"]["en"], "Path of Inquiry");
    assert!(path["name"]["zh_tw"].is_string() && path["name"]["es"].is_string());
}

#[sqlx::test]
async fn creating_an_achievement_is_audited_and_unlocks_for_students_who_already_qualify(
    pool: PgPool,
) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (qualified, close, none) = (
        student(&pool, 1).await,
        student(&pool, 2).await,
        student(&pool, 3).await,
    );
    complete_n(&pool, qualified, 3).await;
    complete_n(&pool, close, 2).await;
    let _ = none;

    let mut body = new_achievement("three_done", "completed_conversations", 3);
    body["points"] = json!(15);
    let (status, created) = call(&app, "POST", LIST, &a.admin, body).await;

    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["achievement"]["id"], "three_done");
    assert_eq!(created["achievement"]["enabled"], true);
    assert_eq!(
        created["achievement"]["levels"],
        json!([{"level": 1, "threshold": 3, "points": 15}])
    );
    assert_eq!(created["retroactive_unlocks"], 1);
    assert!(unlocked(&pool, qualified, "three_done", 1).await);
    assert!(!unlocked(&pool, close, "three_done", 1).await);
    assert_eq!(
        ledger_rows(&pool, qualified, "three_done").await,
        vec![(15, "three_done:1".to_string())]
    );

    let audit: (
        Option<String>,
        String,
        Option<Value>,
        Option<Value>,
        Option<i32>,
    ) = sqlx::query_as(
        "SELECT u.email, a.target_email, a.old_value, a.new_value, a.item_count
         FROM audit_logs a LEFT JOIN users u ON u.id = a.actor_id
         WHERE a.action = 'achievement_created'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit.0.as_deref(), Some("admin@example.com"));
    assert_eq!(audit.1, "three_done");
    assert!(audit.2.is_none());
    assert_eq!(audit.3.as_ref().unwrap()["levels"][0]["threshold"], 3);
    assert_eq!(audit.4, Some(1));
    let leaked = audit.3.unwrap().to_string();
    assert!(
        !leaked.contains("achievement-"),
        "audit must not carry student data"
    );

    // 追溯重跑冪等：不重複解鎖、不重複入帳
    let before = all_ledger_rows(&pool).await;
    assert!(
        achievements::backfill(&pool, "three_done")
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(all_ledger_rows(&pool).await, before);
}

#[sqlx::test]
async fn new_achievement_ignores_students_who_do_not_qualify_and_later_completions_still_unlock(
    pool: PgPool,
) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let early = student(&pool, 1).await;
    complete_n(&pool, early, 1).await;

    let (status, created) = call(
        &app,
        "POST",
        LIST,
        &a.admin,
        new_achievement("two_done", "completed_conversations", 2),
    )
    .await;
    assert_eq!(
        (status, &created["retroactive_unlocks"]),
        (StatusCode::CREATED, &json!(0))
    );
    assert!(!unlocked(&pool, early, "two_done", 1).await);

    complete_n(&pool, early, 1).await;
    assert!(unlocked(&pool, early, "two_done", 1).await);
    assert_eq!(
        ledger_rows(&pool, early, "two_done").await,
        vec![(10, "two_done:1".to_string())],
        "points default to 10"
    );
}

#[sqlx::test]
async fn task_and_purchase_achievements_are_evaluated_from_the_ledger_when_created(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (claimer, buyer) = (student(&pool, 1).await, student(&pool, 2).await);
    let mut conn = pool.acquire().await.unwrap();
    let task = Entry::award(
        claimer,
        std::num::NonZeroU32::new(5).unwrap(),
        "task",
        Source::Task,
        "daily:2026-10-24",
    );
    points::record(&mut conn, &task).await.unwrap();
    let purchase = Entry::purchase(buyer, "part", "hat", 0);
    points::record(&mut conn, &purchase).await.unwrap();
    drop(conn);

    let (s1, r1) = call(
        &app,
        "POST",
        LIST,
        &a.admin,
        new_achievement("claimed", "first_task_claim", 1),
    )
    .await;
    let (s2, r2) = call(
        &app,
        "POST",
        LIST,
        &a.admin,
        new_achievement("bought", "first_purchase", 1),
    )
    .await;

    assert_eq!((s1, s2), (StatusCode::CREATED, StatusCode::CREATED));
    assert_eq!(
        (&r1["retroactive_unlocks"], &r2["retroactive_unlocks"]),
        (&json!(1), &json!(1))
    );
    assert!(unlocked(&pool, claimer, "claimed", 1).await);
    assert!(!unlocked(&pool, buyer, "claimed", 1).await);
    assert!(unlocked(&pool, buyer, "bought", 1).await);
    assert!(!unlocked(&pool, claimer, "bought", 1).await);
}

#[sqlx::test]
async fn lowering_a_threshold_unlocks_qualified_students_but_raising_never_revokes(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (thirty, five) = (student(&pool, 1).await, student(&pool, 2).await);
    complete_n(&pool, thirty, 30).await;
    complete_n(&pool, five, 5).await;
    assert!(!unlocked(&pool, thirty, "dialogue_path", 3).await);
    assert!(unlocked(&pool, five, "dialogue_path", 1).await);

    let (status, lowered) = call(
        &app,
        "PATCH",
        &format!("{LIST}/dialogue_path"),
        &a.admin,
        json!({"levels": [{"level": 3, "threshold": 30}]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{lowered}");
    assert_eq!(lowered["retroactive_unlocks"], 1);
    assert!(unlocked(&pool, thirty, "dialogue_path", 3).await);
    assert!(!unlocked(&pool, five, "dialogue_path", 3).await);

    let (status, raised) = call(
        &app,
        "PATCH",
        &format!("{LIST}/dialogue_path"),
        &a.admin,
        json!({"levels": [{"level": 1, "threshold": 10}]}),
    )
    .await;
    assert_eq!(
        (status, &raised["retroactive_unlocks"]),
        (StatusCode::OK, &json!(0))
    );
    assert!(
        unlocked(&pool, five, "dialogue_path", 1).await,
        "raising keeps earlier unlocks"
    );

    let audit: (Option<Value>, Option<Value>, Option<i32>) = sqlx::query_as(
        "SELECT old_value, new_value, item_count FROM audit_logs
         WHERE action = 'achievement_updated' ORDER BY created_at, id LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit.0.unwrap()["levels"][2]["threshold"], 50);
    assert_eq!(audit.1.unwrap()["levels"][2]["threshold"], 30);
    assert_eq!(audit.2, Some(1));
}

#[sqlx::test]
async fn disabled_achievements_do_not_unlock_keep_existing_unlocks_and_backfill_on_reenable(
    pool: PgPool,
) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (early, later) = (student(&pool, 1).await, student(&pool, 2).await);
    complete_n(&pool, early, 1).await;
    assert!(unlocked(&pool, early, "first_dialogue", 1).await);
    let ledger_before = all_ledger_rows(&pool).await;

    let (status, off) = call(
        &app,
        "PATCH",
        &format!("{LIST}/first_dialogue"),
        &a.admin,
        json!({"enabled": false}),
    )
    .await;
    assert_eq!(
        (status, &off["achievement"]["enabled"]),
        (StatusCode::OK, &json!(false))
    );
    assert!(unlocked(&pool, early, "first_dialogue", 1).await);
    assert_eq!(
        all_ledger_rows(&pool).await,
        ledger_before,
        "disabling pays nothing and takes nothing back"
    );

    complete_n(&pool, later, 1).await;
    assert!(!unlocked(&pool, later, "first_dialogue", 1).await);

    let (status, on) = call(
        &app,
        "PATCH",
        &format!("{LIST}/first_dialogue"),
        &a.admin,
        json!({"enabled": true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(on["retroactive_unlocks"], 1);
    assert!(unlocked(&pool, later, "first_dialogue", 1).await);
    assert_eq!(ledger_rows(&pool, later, "first_dialogue").await.len(), 1);
    assert_eq!(
        ledger_rows(&pool, early, "first_dialogue").await.len(),
        1,
        "no double award"
    );
}

#[sqlx::test]
async fn changing_points_only_affects_later_unlocks(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (first, second) = (student(&pool, 1).await, student(&pool, 2).await);
    complete_n(&pool, first, 1).await;

    let (status, _) = call(
        &app,
        "PATCH",
        &format!("{LIST}/first_dialogue"),
        &a.admin,
        json!({"levels": [{"level": 1, "points": 25}]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    complete_n(&pool, second, 1).await;

    assert_eq!(
        ledger_rows(&pool, first, "first_dialogue").await,
        vec![(10, "first_dialogue:1".into())]
    );
    assert_eq!(
        ledger_rows(&pool, second, "first_dialogue").await,
        vec![(25, "first_dialogue:1".into())]
    );
}

#[sqlx::test]
async fn zero_point_achievement_records_the_unlock_without_a_ledger_row(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let (status, _) = call(
        &app,
        "PATCH",
        &format!("{LIST}/first_dialogue"),
        &a.admin,
        json!({"levels": [{"level": 1, "points": 0}]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let user_id = student(&pool, 1).await;

    complete_n(&pool, user_id, 1).await;

    assert!(unlocked(&pool, user_id, "first_dialogue", 1).await);
    assert!(
        ledger_rows(&pool, user_id, "first_dialogue")
            .await
            .is_empty()
    );
}

#[sqlx::test]
async fn editing_text_and_order_is_audited_and_the_condition_type_is_fixed(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;

    let (status, updated) = call(
        &app,
        "PATCH",
        &format!("{LIST}/sharp_reasoner"),
        &a.admin,
        json!({"name": {"en": "Sharper"}, "description": {"es": "Nueva descripción"}, "sort_order": 42}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["achievement"]["name"]["en"], "Sharper");
    assert_eq!(
        updated["achievement"]["name"]["zh_tw"], "思辨高手",
        "other languages untouched"
    );
    assert_eq!(
        updated["achievement"]["description"]["es"],
        "Nueva descripción"
    );
    assert_eq!(updated["achievement"]["sort_order"], 42);
    assert_eq!(updated["retroactive_unlocks"], 0);

    let (status, _) = call(
        &app,
        "PATCH",
        &format!("{LIST}/sharp_reasoner"),
        &a.admin,
        json!({"condition_type": "consecutive_days"}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "the condition type cannot change"
    );

    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE action = 'achievement_updated' AND target_email = 'sharp_reasoner'
           AND old_value IS NOT NULL AND new_value IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audits, 1, "only the accepted change is audited");
}

#[sqlx::test]
async fn invalid_input_is_rejected_without_side_effects(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let before = all_ledger_rows(&pool).await;

    let mut cases: Vec<(&str, Value)> = vec![
        (
            "unknown condition type",
            new_achievement("x1", "invented", 1),
        ),
        (
            "negative threshold",
            new_achievement("x2", "completed_conversations", -1),
        ),
        (
            "zero threshold",
            new_achievement("x3", "completed_conversations", 0),
        ),
        ("fractional threshold", {
            let mut b = new_achievement("x4", "completed_conversations", 1);
            b["threshold"] = json!(1.5);
            b
        }),
        (
            "dimension score above 5",
            new_achievement("x5", "dimension_score", 6),
        ),
        ("negative points", {
            let mut b = new_achievement("x6", "completed_conversations", 1);
            b["points"] = json!(-1);
            b
        }),
        ("fractional points", {
            let mut b = new_achievement("x7", "completed_conversations", 1);
            b["points"] = json!(2.5);
            b
        }),
        ("blank name", {
            let mut b = new_achievement("x8", "completed_conversations", 1);
            b["name"]["en"] = json!("   ");
            b
        }),
        ("blank description", {
            let mut b = new_achievement("x9", "completed_conversations", 1);
            b["description"]["zh_tw"] = json!("");
            b
        }),
        ("missing language", {
            let mut b = new_achievement("x10", "completed_conversations", 1);
            b["name"].as_object_mut().unwrap().remove("es");
            b
        }),
        (
            "bad id",
            new_achievement("Bad Id", "completed_conversations", 1),
        ),
        (
            "task claim takes no parameter",
            new_achievement("x11", "first_task_claim", 3),
        ),
    ];
    cases.push((
        "duplicate id is a conflict, not a validation error",
        Value::Null,
    ));
    for (label, body) in cases {
        if body.is_null() {
            let (status, _) = call(
                &app,
                "POST",
                LIST,
                &a.admin,
                new_achievement("first_dialogue", "completed_conversations", 9),
            )
            .await;
            assert_eq!(status, StatusCode::CONFLICT, "{label}");
            continue;
        }
        let (status, resp) = call(&app, "POST", LIST, &a.admin, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{label}: {resp}");
    }

    let patches = [
        (
            "levels must strictly increase",
            json!({"levels": [{"level": 1, "threshold": 20}]}),
        ),
        (
            "equal thresholds",
            json!({"levels": [{"level": 2, "threshold": 5}]}),
        ),
        (
            "unknown level",
            json!({"levels": [{"level": 9, "threshold": 100}]}),
        ),
        (
            "negative points",
            json!({"levels": [{"level": 1, "points": -5}]}),
        ),
        ("blank name", json!({"name": {"zh_tw": " "}})),
        ("empty change", json!({})),
    ];
    for (label, body) in patches {
        let (status, resp) = call(
            &app,
            "PATCH",
            &format!("{LIST}/dialogue_path"),
            &a.admin,
            body,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{label}: {resp}");
    }
    let (status, _) = call(
        &app,
        "PATCH",
        &format!("{LIST}/missing"),
        &a.admin,
        json!({"enabled": false}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let ids: i64 = sqlx::query_scalar("SELECT count(DISTINCT id) FROM achievements")
        .fetch_one(&pool)
        .await
        .unwrap();
    let audits: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action LIKE 'achievement_%'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((ids, audits, all_ledger_rows(&pool).await), (6, 0, before));
}

#[sqlx::test]
async fn there_is_no_route_to_delete_an_achievement(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;

    for path in [format!("{LIST}/first_dialogue"), LIST.to_string()] {
        let res = send(&app, json_req("DELETE", &path, Some(&a.admin), None)).await;
        assert!(
            matches!(
                res.status(),
                StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_FOUND
            ),
            "{path}: {}",
            res.status()
        );
    }
    let doc = json_body(send(&app, get("/api/openapi.json", None)).await).await;
    for path in ["/api/admin/achievements", "/api/admin/achievements/{id}"] {
        assert!(doc["paths"][path].get("delete").is_none(), "{path}");
    }
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM achievements")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 8);
}

#[sqlx::test]
async fn database_rejects_levels_whose_thresholds_do_not_increase(pool: PgPool) {
    let err = sqlx::query(
        "UPDATE achievements SET threshold = 5 WHERE id = 'dialogue_path' AND level = 2",
    )
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(err.to_string().contains("strictly increase"), "{err}");

    let mut tx = pool.begin().await.unwrap();
    sqlx::query("UPDATE achievements SET threshold = threshold + 100 WHERE id = 'dialogue_path'")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

#[sqlx::test]
async fn a_streak_achievement_added_later_counts_past_taipei_days(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let user_id = student(&pool, 1).await;
    // Taipei 23:59 Oct 20, 00:01 Oct 21, 12:00 Oct 22: three adjacent calendar days
    let first = Utc.with_ymd_and_hms(2026, 10, 20, 15, 59, 0).unwrap();
    for (i, at) in [
        first,
        first + Duration::minutes(2),
        first + Duration::hours(36),
    ]
    .into_iter()
    .enumerate()
    {
        achievements::record_completed_conversation(&pool, user_id, Uuid::new_v4(), at)
            .await
            .unwrap();
        let _ = i;
    }
    sqlx::query("DELETE FROM achievement_unlocks WHERE achievement_id = 'three_day_streak'")
        .execute(&pool)
        .await
        .unwrap();

    let (status, created) = call(
        &app,
        "POST",
        LIST,
        &a.admin,
        new_achievement("four_days", "consecutive_days", 4),
    )
    .await;
    assert_eq!(
        (status, &created["retroactive_unlocks"]),
        (StatusCode::CREATED, &json!(0))
    );
    let (status, lowered) = call(
        &app,
        "PATCH",
        &format!("{LIST}/four_days"),
        &a.admin,
        json!({"levels": [{"level": 1, "threshold": 3}]}),
    )
    .await;
    assert_eq!(
        (status, &lowered["retroactive_unlocks"]),
        (StatusCode::OK, &json!(1))
    );
}
