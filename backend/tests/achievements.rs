use chrono::{DateTime, Duration, TimeZone, Utc};
use socrates_chat_backend::achievements::{
    Unlock, record_completed_conversation, record_completed_conversation_in_tx,
    record_dimension_score, record_first_character_part, record_first_task_reward,
    record_first_task_reward_in_tx,
};
use sqlx::PgPool;
use uuid::Uuid;

mod common;

async fn user(pool: &PgPool) -> Uuid {
    common::seed_user(pool, "achievement-student@example.com").await
}

async fn complete(
    pool: &PgPool,
    user_id: Uuid,
    conversation_id: Uuid,
    at: DateTime<Utc>,
) -> Vec<Unlock> {
    record_completed_conversation(pool, user_id, conversation_id, at)
        .await
        .unwrap()
}

/// Number of unlock rows and achievement ledger rows the user has.
async fn counts(pool: &PgPool, user_id: Uuid) -> (i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM achievement_unlocks WHERE user_id=$1),
                (SELECT count(*) FROM point_ledger WHERE user_id=$1 AND source_type='achievement')",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn completion_unlocks_once_and_duplicate_events_do_not_award_points(pool: PgPool) {
    let user_id = user(&pool).await;
    let conversation_id = Uuid::new_v4();
    let completed_at = Utc.with_ymd_and_hms(2026, 10, 24, 4, 0, 0).unwrap();

    let first = complete(&pool, user_id, conversation_id, completed_at).await;
    let repeated = complete(&pool, user_id, conversation_id, completed_at).await;

    assert_eq!(first.len(), 1);
    assert_eq!(first[0].achievement_id, "first_dialogue");
    assert_eq!(first[0].points, 10);
    assert!(repeated.is_empty());
    assert_eq!(counts(&pool, user_id).await, (1, 1));
    let award: (i32, String) = sqlx::query_as(
        "SELECT delta, source_id FROM point_ledger
         WHERE user_id = $1 AND source_type = 'achievement'",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(award, (10, "first_dialogue:1".to_string()));
}

#[sqlx::test]
async fn simultaneous_completions_cannot_skip_a_threshold(pool: PgPool) {
    let user_id = user(&pool).await;
    let at = Utc.with_ymd_and_hms(2026, 10, 24, 4, 0, 0).unwrap();
    let (left, right) = tokio::join!(
        complete(&pool, user_id, Uuid::new_v4(), at),
        complete(&pool, user_id, Uuid::new_v4(), at),
    );
    assert_eq!(left.len() + right.len(), 1);
    let events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM achievement_completion_events WHERE user_id=$1")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((events, counts(&pool, user_id).await.0), (2, 1));
}

#[sqlx::test]
async fn completion_unlocks_progress_levels_and_taipei_streak_once(pool: PgPool) {
    let user_id = user(&pool).await;
    let start = Utc.with_ymd_and_hms(2026, 10, 20, 16, 5, 0).unwrap(); // Taipei 00:05
    // 50 completions over three Taipei days: 17 + 17 + 16.
    let mut unlocked_by_completion = Vec::new();
    for i in 0..50 {
        let at = start + Duration::days(i / 17);
        unlocked_by_completion.push(complete(&pool, user_id, Uuid::new_v4(), at).await);
    }
    assert!(
        unlocked_by_completion[4]
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
    for expected in [
        ("dialogue_path", 1),
        ("dialogue_path", 2),
        ("dialogue_path", 3),
        ("three_day_streak", 1),
    ] {
        assert!(
            levels.contains(&(expected.0.into(), expected.1)),
            "{expected:?}"
        );
    }
    let total_unlocks = unlocked_by_completion.iter().map(Vec::len).sum::<usize>();
    assert_eq!(
        total_unlocks,
        levels.len(),
        "each level is reported exactly once"
    );
}

#[sqlx::test]
async fn score_task_and_purchase_signals_are_idempotent(pool: PgPool) {
    let user_id = user(&pool).await;
    // Incomplete or out-of-range score sets never qualify.
    for scores in [&[5; 5][..], &[1, 1, 1, 1, 1, 4], &[1, 1, 1, 1, 1, 6]] {
        assert!(
            record_dimension_score(&pool, user_id, scores)
                .await
                .unwrap()
                .is_empty()
        );
    }
    let scores = [1, 1, 1, 1, 1, 5];
    let results = [
        record_dimension_score(&pool, user_id, &scores).await,
        record_dimension_score(&pool, user_id, &scores).await,
        record_first_task_reward(&pool, user_id).await,
        record_first_task_reward(&pool, user_id).await,
        record_first_character_part(&pool, user_id).await,
        record_first_character_part(&pool, user_id).await,
    ];
    let unlocked: Vec<usize> = results.iter().map(|r| r.as_ref().unwrap().len()).collect();
    assert_eq!(unlocked, [1, 0, 1, 0, 1, 0]);
    assert_eq!(counts(&pool, user_id).await, (3, 3));
}

#[sqlx::test]
async fn event_transaction_rollback_also_rolls_back_achievement_and_points(pool: PgPool) {
    let user_id = user(&pool).await;
    let at = Utc.with_ymd_and_hms(2026, 10, 24, 4, 0, 0).unwrap();
    let mut tx = pool.begin().await.unwrap();
    let from_reward = record_first_task_reward_in_tx(&mut tx, user_id)
        .await
        .unwrap();
    let from_completion = record_completed_conversation_in_tx(&mut tx, user_id, Uuid::new_v4(), at)
        .await
        .unwrap();
    assert_eq!((from_reward.len(), from_completion.len()), (1, 1));
    tx.rollback().await.unwrap();
    assert_eq!(counts(&pool, user_id).await, (0, 0));
}

#[sqlx::test]
async fn account_deletion_cascades_to_unlocks(pool: PgPool) {
    let user_id = user(&pool).await;
    complete(&pool, user_id, Uuid::new_v4(), Utc::now()).await;
    sqlx::query("DELETE FROM users WHERE id=$1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(counts(&pool, user_id).await.0, 0);
}

async fn streak_unlocks(pool: &PgPool, user_id: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM achievement_unlocks
         WHERE user_id = $1 AND achievement_id = 'three_day_streak'",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn a_missed_day_resets_the_streak_until_three_adjacent_days_follow(pool: PgPool) {
    let user_id = user(&pool).await;
    // Taipei days 1, 2, 4: the gap on day 3 means no streak of three yet.
    let day = |n: i64| Utc.with_ymd_and_hms(2026, 10, 1, 4, 0, 0).unwrap() + Duration::days(n - 1);
    for n in [1, 2, 4] {
        complete(&pool, user_id, Uuid::new_v4(), day(n)).await;
    }
    assert_eq!(streak_unlocks(&pool, user_id).await, 0);

    complete(&pool, user_id, Uuid::new_v4(), day(5)).await;
    assert_eq!(
        streak_unlocks(&pool, user_id).await,
        0,
        "days 4 and 5 are only two"
    );
    let sixth = complete(&pool, user_id, Uuid::new_v4(), day(6)).await;
    assert!(sixth.iter().any(|u| u.achievement_id == "three_day_streak"));

    // A later break neither revokes nor unlocks it again.
    let after_gap = complete(&pool, user_id, Uuid::new_v4(), day(20)).await;
    assert!(after_gap.is_empty());
    assert_eq!(streak_unlocks(&pool, user_id).await, 1);
}

#[sqlx::test]
async fn streak_days_are_split_at_taipei_midnight_not_utc_midnight(pool: PgPool) {
    let user_id = user(&pool).await;
    // 15:30 and 16:30 UTC are on the same UTC date but either side of Taipei midnight.
    let late = Utc.with_ymd_and_hms(2026, 10, 20, 15, 59, 0).unwrap(); // Taipei 23:59
    let after_midnight = late + Duration::minutes(2); // Taipei 00:01 next day
    complete(&pool, user_id, Uuid::new_v4(), late).await;
    complete(&pool, user_id, Uuid::new_v4(), after_midnight).await;
    assert_eq!(
        streak_unlocks(&pool, user_id).await,
        0,
        "two adjacent days are not three"
    );

    let third = complete(
        &pool,
        user_id,
        Uuid::new_v4(),
        after_midnight + Duration::days(1),
    )
    .await;
    assert!(third.iter().any(|u| u.achievement_id == "three_day_streak"));

    // Two completions inside one Taipei day count as a single day.
    let other = common::seed_user(&pool, "same-day@example.com").await;
    let morning = Utc.with_ymd_and_hms(2026, 10, 20, 16, 5, 0).unwrap(); // Taipei 00:05
    for at in [
        morning,
        morning + Duration::hours(20),
        morning + Duration::days(1),
    ] {
        complete(&pool, other, Uuid::new_v4(), at).await;
    }
    assert_eq!(
        streak_unlocks(&pool, other).await,
        0,
        "two distinct days only"
    );
}

#[sqlx::test]
async fn completion_events_do_not_depend_on_the_conversation_still_existing(pool: PgPool) {
    let referenced: Vec<String> = sqlx::query_scalar(
        "SELECT ccu.table_name::text
         FROM information_schema.table_constraints tc
         JOIN information_schema.constraint_column_usage ccu ON ccu.constraint_name = tc.constraint_name
         WHERE tc.table_name = 'achievement_completion_events' AND tc.constraint_type = 'FOREIGN KEY'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        referenced,
        vec!["users".to_string()],
        "deleting a conversation must not delete its completion event or unlocks"
    );
}

#[sqlx::test]
async fn zero_point_unlock_writes_no_ledger_row(pool: PgPool) {
    sqlx::query("UPDATE achievements SET points = 0 WHERE id = 'first_dialogue'")
        .execute(&pool)
        .await
        .unwrap();
    let user_id = user(&pool).await;

    let unlocked = complete(&pool, user_id, Uuid::new_v4(), Utc::now()).await;

    assert_eq!(unlocked.len(), 1);
    assert_eq!(unlocked[0].points, 0);
    assert_eq!(counts(&pool, user_id).await, (1, 0));
}
