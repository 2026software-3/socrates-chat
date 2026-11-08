//! F-19.3：任務進度、領取與防重複，以合成資料驗證。
mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test]
async fn student_claims_completed_daily_task_once_and_unlocks_first_reward(pool: PgPool) {
    let app = test_app(pool.clone());
    let actors = seed_actors(&app).await;
    let user_id: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email = 'student@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    socrates_chat_backend::achievements::record_completed_conversation(
        &pool,
        user_id,
        Uuid::new_v4(),
        chrono::Utc::now(),
    )
    .await
    .unwrap();

    let res = send(&app, get("/api/tasks", Some(&actors.student))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let tasks = json_body(res).await;
    let daily = tasks
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["id"] == "daily_dialogue")
        .unwrap();
    assert_eq!(daily["progress"], 1);
    assert_eq!(daily["claimed"], false);
    let start = daily["period_start"].as_str().unwrap();
    let body = json!({"period_start": start});
    let claim = send(
        &app,
        json_req(
            "POST",
            "/api/tasks/daily_dialogue/claim",
            Some(&actors.student),
            Some(body.clone()),
        ),
    )
    .await;
    assert_eq!(claim.status(), StatusCode::OK);
    let repeated = send(
        &app,
        json_req(
            "POST",
            "/api/tasks/daily_dialogue/claim",
            Some(&actors.student),
            Some(body),
        ),
    )
    .await;
    assert_eq!(repeated.status(), StatusCode::CONFLICT);
    let rewards: Vec<(String, i32)> = sqlx::query_as("SELECT source_type, delta FROM point_ledger WHERE user_id = $1 ORDER BY source_type, delta")
        .bind(user_id).fetch_all(&pool).await.unwrap();
    assert_eq!(
        rewards,
        vec![
            ("achievement".into(), 10),
            ("achievement".into(), 10),
            ("task".into(), 5)
        ]
    );
}

#[sqlx::test]
async fn tasks_require_student_and_admin_configuration_requires_admin(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    for (cookie, status) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        (Some(a.teacher.as_str()), StatusCode::FORBIDDEN),
    ] {
        assert_eq!(send(&app, get("/api/tasks", cookie)).await.status(), status);
        assert_eq!(
            send(
                &app,
                json_req(
                    "POST",
                    "/api/tasks/daily_dialogue/claim",
                    cookie,
                    Some(json!({"period_start":"2026-01-01"}))
                )
            )
            .await
            .status(),
            status
        );
    }
    for cookie in [a.student.as_str(), a.teacher.as_str(), a.outsider.as_str()] {
        assert_eq!(
            send(&app, get("/api/admin/tasks", Some(cookie)))
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            send(
                &app,
                json_req(
                    "PATCH",
                    "/api/admin/tasks/daily_dialogue",
                    Some(cookie),
                    Some(json!({"goal":2,"reward":8}))
                )
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        send(&app, get("/api/admin/tasks", None)).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test]
async fn incomplete_expired_and_duplicate_claims_do_not_pay(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let daily = json_body(send(&app, get("/api/tasks", Some(&a.student))).await).await;
    let start = daily
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["id"] == "daily_dialogue")
        .unwrap()["period_start"]
        .as_str()
        .unwrap();
    let body = json!({"period_start":start});
    assert_eq!(
        send(
            &app,
            json_req(
                "POST",
                "/api/tasks/daily_dialogue/claim",
                Some(&a.student),
                Some(body.clone())
            )
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(
            &app,
            json_req(
                "POST",
                "/api/tasks/daily_dialogue/claim",
                Some(&a.student),
                Some(json!({"period_start":"2020-01-01"}))
            )
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM point_ledger WHERE source_type='task'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    let user_id: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email = 'student@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    socrates_chat_backend::achievements::record_completed_conversation(
        &pool,
        user_id,
        Uuid::new_v4(),
        chrono::Utc::now(),
    )
    .await
    .unwrap();
    let one = send(
        &app,
        json_req(
            "POST",
            "/api/tasks/daily_dialogue/claim",
            Some(&a.student),
            Some(body.clone()),
        ),
    );
    let two = send(
        &app,
        json_req(
            "POST",
            "/api/tasks/daily_dialogue/claim",
            Some(&a.student),
            Some(body),
        ),
    );
    let (one, two) = tokio::join!(one, two);
    let mut statuses = [one.status(), two.status()];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::CONFLICT]);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM point_ledger WHERE source_type='task' AND user_id=$1",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test]
async fn admin_updates_task_and_audits_changes(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    for body in [json!({"goal":0,"reward":8}), json!({"goal":1,"reward":0})] {
        assert_eq!(
            send(
                &app,
                json_req(
                    "PATCH",
                    "/api/admin/tasks/daily_dialogue",
                    Some(&a.admin),
                    Some(body)
                )
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let res = send(
        &app,
        json_req(
            "PATCH",
            "/api/admin/tasks/daily_dialogue",
            Some(&a.admin),
            Some(json!({"goal":2,"reward":8})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    assert_eq!(
        (body["goal"].as_i64(), body["reward"].as_i64()),
        (Some(2), Some(8))
    );
    let audit: (serde_json::Value, serde_json::Value) = sqlx::query_as("SELECT old_value, new_value FROM audit_logs WHERE action='task_updated' AND target_email='daily_dialogue'").fetch_one(&pool).await.unwrap();
    assert_eq!(
        (audit.0["reward"].as_i64(), audit.1["reward"].as_i64()),
        (Some(5), Some(8))
    );
}

#[sqlx::test]
async fn daily_progress_uses_taipei_midnight_and_weekly_task_can_be_claimed(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let user_id: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email='student@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query("INSERT INTO achievement_completion_events (user_id, conversation_id, completed_at) SELECT $1, gen_random_uuid(), (((clock_timestamp() AT TIME ZONE 'Asia/Taipei')::date - 1)::timestamp AT TIME ZONE 'Asia/Taipei') + interval '23 hours 59 minutes 59 seconds'")
        .bind(user_id).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO achievement_completion_events (user_id, conversation_id, completed_at) SELECT $1, gen_random_uuid(), ((clock_timestamp() AT TIME ZONE 'Asia/Taipei')::date::timestamp AT TIME ZONE 'Asia/Taipei') FROM generate_series(1,5)")
        .bind(user_id).execute(&pool).await.unwrap();
    let list = json_body(send(&app, get("/api/tasks", Some(&a.student))).await).await;
    let daily = list
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["id"] == "daily_dialogue")
        .unwrap();
    assert_eq!(
        daily["progress"], 5,
        "the event before Taipei midnight belongs to the previous day"
    );
    let weekly = list
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["id"] == "weekly_dialogue")
        .unwrap();
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/tasks/weekly_dialogue/claim",
            Some(&a.student),
            Some(json!({"period_start":weekly["period_start"]})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let reward: i32 = sqlx::query_scalar(
        "SELECT delta FROM point_ledger WHERE user_id=$1 AND source_type='task'",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reward, 20);
}

#[sqlx::test]
async fn failed_achievement_award_rolls_back_the_task_claim(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let user_id: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email='student@example.com'")
            .fetch_one(&pool)
            .await
            .unwrap();
    socrates_chat_backend::achievements::record_completed_conversation(
        &pool,
        user_id,
        Uuid::new_v4(),
        chrono::Utc::now(),
    )
    .await
    .unwrap();
    sqlx::raw_sql("CREATE FUNCTION reject_task_achievement() RETURNS trigger AS $$ BEGIN IF NEW.achievement_id = 'first_task_reward' THEN RAISE EXCEPTION 'synthetic error'; END IF; RETURN NEW; END $$ LANGUAGE plpgsql; CREATE TRIGGER reject_task_achievement BEFORE INSERT ON achievement_unlocks FOR EACH ROW EXECUTE FUNCTION reject_task_achievement();").execute(&pool).await.unwrap();
    let list = json_body(send(&app, get("/api/tasks", Some(&a.student))).await).await;
    let daily = list
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["id"] == "daily_dialogue")
        .unwrap();
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/tasks/daily_dialogue/claim",
            Some(&a.student),
            Some(json!({"period_start":daily["period_start"]})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM point_ledger WHERE user_id=$1 AND source_type='task'",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 0);
}

#[sqlx::test(migrations = false)]
async fn task_migration_upgrades_existing_ledger_and_preserves_audit_actions(pool: PgPool) {
    let migrator = sqlx::migrate!();
    migrator.run_to(20261107000300, &pool).await.unwrap();
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (email) VALUES ('prior-task@example.com') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO point_ledger (user_id, delta, reason, source_type, source_id) VALUES ($1, 10, 'test', 'admin_adjustment', 'prior')").bind(user_id).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO audit_logs (actor_id, action, target_email) VALUES ($1, 'achievement_created', 'example')").bind(user_id).execute(&pool).await.unwrap();
    migrator.run(&pool).await.unwrap();
    migrator.run(&pool).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM point_ledger WHERE user_id=$1")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
    sqlx::query("INSERT INTO audit_logs (actor_id, action, target_email) VALUES ($1, 'achievement_updated', 'example'), ($1, 'task_updated', 'daily_dialogue')").bind(user_id).execute(&pool).await.unwrap();
}
