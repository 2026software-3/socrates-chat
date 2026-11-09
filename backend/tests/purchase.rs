//! F-19.4：點數購買以合成部件驗證。
mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

async fn seed(pool: &PgPool, price: i32, enabled: bool) {
    sqlx::query("INSERT INTO store_parts (id, name_zh_tw, name_en, name_es, price, enabled) VALUES ('synthetic_part', '測試部件', 'Test part', 'Pieza de prueba', $1, $2)")
        .bind(price).bind(enabled).execute(pool).await.unwrap();
}

#[sqlx::test]
async fn purchasing_with_sufficient_balance_unlocks_part_and_achievement_once(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let user_id: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email='student@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    seed(&pool, 30, true).await;
    sqlx::query("INSERT INTO point_ledger (user_id, delta, reason, source_type, source_id) VALUES ($1, 50, 'test', 'admin_adjustment', 'synthetic')").bind(user_id).execute(&pool).await.unwrap();
    let parts = send(&app, get("/api/parts", Some(&a.student))).await;
    assert_eq!(parts.status(), StatusCode::OK);
    assert_eq!(json_body(parts).await[0]["owned"], false);
    let first = send(
        &app,
        json_req(
            "POST",
            "/api/parts/synthetic_part/purchase",
            Some(&a.student),
            None,
        ),
    )
    .await;
    assert_eq!(first.status(), StatusCode::OK);
    let again = send(
        &app,
        json_req(
            "POST",
            "/api/parts/synthetic_part/purchase",
            Some(&a.student),
            None,
        ),
    )
    .await;
    assert_eq!(again.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(again).await["error"]["code"], "part_owned");
    let entries: Vec<(String, i32)> = sqlx::query_as(
        "SELECT source_type, delta FROM point_ledger WHERE user_id=$1 ORDER BY source_type",
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        entries,
        vec![
            ("achievement".into(), 10),
            ("admin_adjustment".into(), 50),
            ("purchase".into(), -30)
        ]
    );
    let parts = json_body(send(&app, get("/api/parts", Some(&a.student))).await).await;
    assert_eq!(parts[0]["owned"], true);
}

#[sqlx::test]
async fn insufficient_disabled_and_other_user_requests_do_not_purchase(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let user_id: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email='student@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    seed(&pool, 30, true).await;
    let attempt = send(
        &app,
        json_req(
            "POST",
            "/api/parts/synthetic_part/purchase",
            Some(&a.student),
            None,
        ),
    )
    .await;
    assert_eq!(attempt.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(attempt).await["error"]["code"],
        "insufficient_points"
    );
    sqlx::query("UPDATE store_parts SET enabled=false WHERE id='synthetic_part'")
        .execute(&pool)
        .await
        .unwrap();
    let attempt = send(
        &app,
        json_req(
            "POST",
            "/api/parts/synthetic_part/purchase",
            Some(&a.student),
            None,
        ),
    )
    .await;
    assert_eq!(attempt.status(), StatusCode::NOT_FOUND);
    for (cookie, status) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        (Some(a.teacher.as_str()), StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            send(
                &app,
                json_req("POST", "/api/parts/synthetic_part/purchase", cookie, None)
            )
            .await
            .status(),
            status
        );
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM point_ledger WHERE user_id=$1 AND source_type IN ('purchase', 'achievement')").bind(user_id).fetch_one(&pool).await.unwrap();
    assert_eq!(count, 0);
}

#[sqlx::test]
async fn simultaneous_purchases_cannot_spend_the_same_balance_twice(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let user_id: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email='student@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    seed(&pool, 30, true).await;
    sqlx::query("INSERT INTO store_parts (id, name_zh_tw, name_en, name_es, price, enabled) VALUES ('second', '二號', 'Second', 'Segunda', 30, true)").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO point_ledger (user_id, delta, reason, source_type, source_id) VALUES ($1, 30, 'test', 'admin_adjustment', 'synthetic')").bind(user_id).execute(&pool).await.unwrap();
    let left = send(
        &app,
        json_req(
            "POST",
            "/api/parts/synthetic_part/purchase",
            Some(&a.student),
            None,
        ),
    );
    let right = send(
        &app,
        json_req("POST", "/api/parts/second/purchase", Some(&a.student), None),
    );
    let (left, right) = tokio::join!(left, right);
    let mut statuses = [left.status(), right.status()];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::CONFLICT]);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM point_ledger WHERE user_id=$1 AND source_type='purchase'",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test]
async fn only_admin_can_change_price_and_status_and_changes_are_audited(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    seed(&pool, 30, true).await;
    let negative = send(
        &app,
        json_req(
            "PATCH",
            "/api/admin/parts/synthetic_part",
            Some(&a.admin),
            Some(json!({"price": -1, "enabled": true})),
        ),
    )
    .await;
    assert_eq!(negative.status(), StatusCode::BAD_REQUEST);
    let audits_before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action='part_updated'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(audits_before, 0);
    let body = json!({"price": 12, "enabled": false});
    for cookie in [&a.student, &a.teacher, &a.outsider] {
        assert_eq!(
            send(
                &app,
                json_req(
                    "PATCH",
                    "/api/admin/parts/synthetic_part",
                    Some(cookie),
                    Some(body.clone())
                )
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        send(
            &app,
            json_req(
                "PATCH",
                "/api/admin/parts/synthetic_part",
                None,
                Some(body.clone())
            )
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let result = send(
        &app,
        json_req(
            "PATCH",
            "/api/admin/parts/synthetic_part",
            Some(&a.admin),
            Some(body),
        ),
    )
    .await;
    assert_eq!(result.status(), StatusCode::OK);
    let audit: (serde_json::Value, serde_json::Value) = sqlx::query_as("SELECT old_value, new_value FROM audit_logs WHERE action='part_updated' AND target_email='synthetic_part'").fetch_one(&pool).await.unwrap();
    assert_eq!(audit.0["price"], 30);
    assert_eq!(audit.1["price"], 12);
}

#[sqlx::test]
async fn free_purchase_is_recorded_and_owned_part_remains_visible_after_disable(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let user_id: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email='student@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    seed(&pool, 0, true).await;
    assert_eq!(
        send(
            &app,
            json_req(
                "POST",
                "/api/parts/synthetic_part/purchase",
                Some(&a.student),
                None
            )
        )
        .await
        .status(),
        StatusCode::OK
    );
    let price: (i32, Option<i32>) = sqlx::query_as(
        "SELECT delta, unit_price FROM point_ledger WHERE user_id=$1 AND source_type='purchase'",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(price, (0, Some(0)));
    sqlx::query("UPDATE store_parts SET enabled=false, price=99 WHERE id='synthetic_part'")
        .execute(&pool)
        .await
        .unwrap();
    let parts = json_body(send(&app, get("/api/parts", Some(&a.student))).await).await;
    assert_eq!(parts[0]["owned"], true);
    assert_eq!(parts[0]["enabled"], false);
}

#[sqlx::test]
async fn failed_achievement_unlock_rolls_back_purchase(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let user_id: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email='student@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    seed(&pool, 0, true).await;
    sqlx::raw_sql("CREATE FUNCTION reject_part_achievement() RETURNS trigger AS $$ BEGIN IF NEW.achievement_id = 'first_character_part' THEN RAISE EXCEPTION 'synthetic error'; END IF; RETURN NEW; END $$ LANGUAGE plpgsql; CREATE TRIGGER reject_part_achievement BEFORE INSERT ON achievement_unlocks FOR EACH ROW EXECUTE FUNCTION reject_part_achievement();").execute(&pool).await.unwrap();
    let response = send(
        &app,
        json_req(
            "POST",
            "/api/parts/synthetic_part/purchase",
            Some(&a.student),
            None,
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM point_ledger WHERE user_id=$1 AND source_type='purchase'",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 0);
}

#[sqlx::test(migrations = false)]
async fn migration_preserves_existing_audit_actions_and_records(pool: PgPool) {
    let migrator = sqlx::migrate!();
    migrator.run_to(20261107000300, &pool).await.unwrap();
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (email) VALUES ('purchase-upgrade@example.com') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO audit_logs (actor_id, action, target_email) VALUES ($1, 'achievement_created', 'synthetic')").bind(user_id).execute(&pool).await.unwrap();
    // Simulate another deployed slice adding its own audit action before this migration.
    sqlx::raw_sql("DO $$ DECLARE old_rule text; BEGIN SELECT pg_get_expr(conbin, conrelid) INTO STRICT old_rule FROM pg_constraint WHERE conrelid='audit_logs'::regclass AND conname='audit_logs_action_check'; ALTER TABLE audit_logs DROP CONSTRAINT audit_logs_action_check; EXECUTE format('ALTER TABLE audit_logs ADD CONSTRAINT audit_logs_action_check CHECK ((%s) OR action = %L)', old_rule, 'task_updated'); END $$;").execute(&pool).await.unwrap();
    migrator.run(&pool).await.unwrap();
    migrator.run(&pool).await.unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action='achievement_created'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
    sqlx::query("INSERT INTO audit_logs (actor_id, action, target_email) VALUES ($1, 'achievement_updated', 'synthetic'), ($1, 'part_updated', 'synthetic'), ($1, 'task_updated', 'synthetic')").bind(user_id).execute(&pool).await.unwrap();
}
