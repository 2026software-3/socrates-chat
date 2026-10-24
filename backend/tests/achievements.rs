use chrono::{Duration, TimeZone, Utc};
use socrates_chat_backend::achievements::{
    record_completed_conversation, record_dimension_score, record_first_character_part,
    record_first_task_reward,
};
use sqlx::PgPool;

async fn user(pool: &PgPool) -> uuid::Uuid {
    sqlx::query_scalar(
        "INSERT INTO users (email) VALUES ('achievement-student@example.com') RETURNING id",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn completion_unlocks_once_and_duplicate_events_do_not_award_points(pool: PgPool) {
    let user_id = user(&pool).await;
    let conversation_id = uuid::Uuid::new_v4();
    let completed_at = Utc.with_ymd_and_hms(2026, 10, 24, 4, 0, 0).unwrap();

    let first = record_completed_conversation(&pool, user_id, conversation_id, completed_at)
        .await
        .unwrap();
    let repeated = record_completed_conversation(&pool, user_id, conversation_id, completed_at)
        .await
        .unwrap();

    assert_eq!(first.len(), 1);
    assert_eq!(first[0].achievement_id, "first_dialogue");
    assert_eq!(first[0].points, 10);
    assert!(repeated.is_empty());
    let (unlocks, awards): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM achievement_unlocks WHERE user_id=$1),
                (SELECT count(*) FROM point_ledger WHERE user_id=$1 AND source_type='achievement')",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((unlocks, awards), (1, 1));
}

#[sqlx::test]
async fn simultaneous_completions_cannot_skip_a_threshold(pool: PgPool) {
    let user_id = user(&pool).await;
    let at = Utc.with_ymd_and_hms(2026, 10, 24, 4, 0, 0).unwrap();
    let (left, right) = tokio::join!(
        record_completed_conversation(&pool, user_id, uuid::Uuid::new_v4(), at),
        record_completed_conversation(&pool, user_id, uuid::Uuid::new_v4(), at),
    );
    assert_eq!(left.unwrap().len() + right.unwrap().len(), 1);
    let (events, unlocks): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM achievement_completion_events WHERE user_id=$1),
                (SELECT count(*) FROM achievement_unlocks WHERE user_id=$1)",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((events, unlocks), (2, 1));
}

#[sqlx::test]
async fn completion_unlocks_progress_levels_and_taipei_streak_once(pool: PgPool) {
    let user_id = user(&pool).await;
    let start = Utc.with_ymd_and_hms(2026, 10, 20, 16, 5, 0).unwrap(); // Taipei 00:05
    let mut at_five = vec![];
    for i in 0..50 {
        let day = i / 17;
        let at = start + Duration::days(day as i64);
        let result = record_completed_conversation(&pool, user_id, uuid::Uuid::new_v4(), at)
            .await
            .unwrap();
        if i == 4 {
            at_five = result;
        }
    }
    assert!(
        at_five
            .iter()
            .any(|u| u.achievement_id == "dialogue_path" && u.level == 1)
    );
    let levels: Vec<(String, i32)> = sqlx::query_as(
        "SELECT achievement_id, level FROM achievement_unlocks WHERE user_id=$1 ORDER BY achievement_id, level",
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(levels.contains(&("dialogue_path".into(), 1)));
    assert!(levels.contains(&("dialogue_path".into(), 2)));
    assert!(levels.contains(&("dialogue_path".into(), 3)));
    assert!(levels.contains(&("three_day_streak".into(), 1)));
}

#[sqlx::test]
async fn score_task_and_purchase_signals_are_idempotent(pool: PgPool) {
    let user_id = user(&pool).await;
    assert!(
        record_dimension_score(&pool, user_id, &[5; 5])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        record_dimension_score(&pool, user_id, &[1, 1, 1, 1, 1, 4])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        record_dimension_score(&pool, user_id, &[1, 1, 1, 1, 1, 6])
            .await
            .unwrap()
            .is_empty()
    );
    let scores = [1, 1, 1, 1, 1, 5];
    assert_eq!(
        record_dimension_score(&pool, user_id, &scores)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        record_dimension_score(&pool, user_id, &scores)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        record_first_task_reward(&pool, user_id)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        record_first_task_reward(&pool, user_id)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        record_first_character_part(&pool, user_id)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        record_first_character_part(&pool, user_id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[sqlx::test]
async fn event_transaction_rollback_also_rolls_back_achievement_and_points(pool: PgPool) {
    let user_id = user(&pool).await;
    let mut tx = pool.begin().await.unwrap();
    socrates_chat_backend::achievements::record_first_task_reward_in_tx(&mut tx, user_id)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    let (unlocks, awards): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM achievement_unlocks WHERE user_id=$1),
                (SELECT count(*) FROM point_ledger WHERE user_id=$1 AND source_type='achievement')",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((unlocks, awards), (0, 0));
}

#[sqlx::test]
async fn achievement_migration_has_catalog_constraints_and_cascades_with_account(pool: PgPool) {
    let user_id = user(&pool).await;
    record_first_task_reward(&pool, user_id).await.unwrap();
    sqlx::query("DELETE FROM users WHERE id=$1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
    let remaining: i64 =
        sqlx::query_scalar("SELECT count(*) FROM achievement_unlocks WHERE user_id=$1")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(remaining, 0);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM achievements")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 8);
}
