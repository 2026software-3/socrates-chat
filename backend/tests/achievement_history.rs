//! Historical conversation rewards seed achievement completion events and unlocks.
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test(migrations = false)]
async fn historical_conversation_rewards_create_events_and_award_met_achievements(pool: PgPool) {
    sqlx::raw_sql(include_str!(
        "../migrations/20260930000000_create_auth_tables.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../migrations/20261013000000_create_point_ledger.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let user_id: Uuid =
        sqlx::query_scalar("INSERT INTO users (email) VALUES ('history@example.com') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    for (day, n) in [
        ("2026-10-01T16:00:00Z", 2),
        ("2026-10-02T16:00:00Z", 2),
        ("2026-10-03T16:00:00Z", 1),
    ] {
        for _ in 0..n {
            sqlx::query("INSERT INTO point_ledger (user_id, delta, reason, source_type, source_id, created_at) VALUES ($1, 10, 'conversation_completed', 'conversation', $2, $3::timestamptz)")
                .bind(user_id).bind(Uuid::new_v4().to_string()).bind(day).execute(&pool).await.unwrap();
        }
    }
    sqlx::raw_sql(include_str!(
        "../migrations/20261107000200_create_achievements.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM achievement_completion_events WHERE user_id=$1")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(events, 5);
    let (conversation_id, completed_at): (Uuid, chrono::DateTime<chrono::Utc>) = sqlx::query_as(
        "SELECT conversation_id, completed_at FROM achievement_completion_events WHERE user_id = $1 LIMIT 1",
    ).bind(user_id).fetch_one(&pool).await.unwrap();
    assert!(
        socrates_chat_backend::achievements::record_completed_conversation(
            &pool,
            user_id,
            conversation_id,
            completed_at,
        )
        .await
        .unwrap()
        .is_empty()
    );
    let awards: Vec<(String, i32)> = sqlx::query_as("SELECT source_id, delta FROM point_ledger WHERE user_id=$1 AND source_type='achievement' ORDER BY source_id")
        .bind(user_id).fetch_all(&pool).await.unwrap();
    assert_eq!(
        awards,
        vec![
            ("dialogue_path:1".into(), 10),
            ("first_dialogue:1".into(), 10),
            ("three_day_streak:1".into(), 10)
        ]
    );
}

#[sqlx::test(migrations = false)]
async fn upgrade_from_deployed_schema_preserves_audit_and_backfills_achievements(pool: PgPool) {
    let migrator = sqlx::migrate!();
    migrator.run_to(20261107000100, &pool).await.unwrap();
    let already_has_column: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'audit_logs' AND column_name = 'old_value')",
    ).fetch_one(&pool).await.unwrap();
    assert!(
        !already_has_column,
        "deployed audit schema must be unchanged"
    );
    let user_id: Uuid =
        sqlx::query_scalar("INSERT INTO users (email) VALUES ('upgrade@example.com') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query("INSERT INTO audit_logs (actor_id, action, target_email) VALUES ($1, 'login', 'upgrade@example.com')")
        .bind(user_id).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO point_ledger (user_id, delta, reason, source_type, source_id) VALUES ($1, 10, 'conversation_completed', 'conversation', $2)")
        .bind(user_id).bind(Uuid::new_v4().to_string()).execute(&pool).await.unwrap();
    migrator.run(&pool).await.unwrap();
    migrator.run(&pool).await.unwrap();
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_logs WHERE action = 'login' AND target_email = 'upgrade@example.com'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(audit, 1);
    let achievements: i64 = sqlx::query_scalar("SELECT count(*) FROM achievement_unlocks WHERE user_id=$1 AND achievement_id='first_dialogue'")
        .bind(user_id).fetch_one(&pool).await.unwrap();
    assert_eq!(achievements, 1);
    sqlx::query("INSERT INTO audit_logs (actor_id, action, target_email, new_value) VALUES ($1, 'achievement_created', 'first_dialogue', '{}'::jsonb)")
        .bind(user_id).execute(&pool).await.unwrap();
}
