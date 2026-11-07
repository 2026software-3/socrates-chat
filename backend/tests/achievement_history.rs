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
        "../migrations/20261025000000_create_achievements.sql"
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
