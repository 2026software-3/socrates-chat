//! Achievement evaluation and idempotent unlocks (F-18.1, S-07.3).
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow, PartialEq, Eq)]
pub struct Unlock {
    pub achievement_id: String,
    pub level: i32,
    pub points: i32,
}

/// Record a summarized conversation exactly once, then award every newly met
/// completion-count or Taipei-streak level atomically with its points.
pub async fn record_completed_conversation(
    pool: &PgPool,
    user_id: Uuid,
    conversation_id: Uuid,
    completed_at: DateTime<Utc>,
) -> Result<Vec<Unlock>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let unlocked =
        record_completed_conversation_in_tx(&mut tx, user_id, conversation_id, completed_at)
            .await?;
    tx.commit().await?;
    Ok(unlocked)
}

/// Variant for callers that must commit completion and awards atomically.
pub async fn record_completed_conversation_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    conversation_id: Uuid,
    completed_at: DateTime<Utc>,
) -> Result<Vec<Unlock>, sqlx::Error> {
    // Serialize a student's progress evaluation so concurrent completions
    // cannot both observe a stale count and skip a threshold permanently.
    sqlx::query("SELECT id FROM users WHERE id = $1 FOR UPDATE")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    let inserted = sqlx::query(
        "INSERT INTO achievement_completion_events (user_id, conversation_id, completed_at)
         VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(conversation_id)
    .bind(completed_at)
    .execute(&mut **tx)
    .await?
    .rows_affected()
        == 1;
    if !inserted {
        return Ok(vec![]);
    }

    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM achievement_completion_events WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&mut **tx)
            .await?;
    let streak: i64 = sqlx::query_scalar(
        "WITH days AS (
            SELECT DISTINCT (completed_at AT TIME ZONE 'Asia/Taipei')::date AS d
            FROM achievement_completion_events WHERE user_id = $1
         ), ordered AS (
            SELECT d, d - (row_number() OVER (ORDER BY d))::int AS grp FROM days
         )
         SELECT COALESCE(max(n), 0)::bigint FROM (
            SELECT count(*) AS n FROM ordered GROUP BY grp
         ) streaks",
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await?;
    let mut unlocked = Vec::new();
    unlocked.extend(unlock_matching(tx, user_id, "completed_conversations", count).await?);
    unlocked.extend(unlock_matching(tx, user_id, "consecutive_days", streak).await?);
    Ok(unlocked)
}

/// Call after the six persisted radar scores are available.
pub async fn record_dimension_score(
    pool: &PgPool,
    user_id: Uuid,
    scores: &[i32],
) -> Result<Vec<Unlock>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let unlocked = record_dimension_score_in_tx(&mut tx, user_id, scores).await?;
    tx.commit().await?;
    Ok(unlocked)
}

/// Transaction-aware score hook. Incomplete or invalid score sets do not qualify.
pub async fn record_dimension_score_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    scores: &[i32],
) -> Result<Vec<Unlock>, sqlx::Error> {
    if scores.len() != 6 || scores.iter().any(|score| !(0..=5).contains(score)) {
        return Ok(vec![]);
    }
    unlock_matching(
        tx,
        user_id,
        "dimension_score",
        i64::from(*scores.iter().max().unwrap()),
    )
    .await
}

/// Call only after a task reward has successfully been claimed.
pub async fn record_first_task_reward(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<Unlock>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let unlocked = record_first_task_reward_in_tx(&mut tx, user_id).await?;
    tx.commit().await?;
    Ok(unlocked)
}

/// Transaction-aware task hook; call in the reward-claim transaction.
pub async fn record_first_task_reward_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<Vec<Unlock>, sqlx::Error> {
    unlock_matching(tx, user_id, "first_task_claim", 1).await
}

/// Call only after a character part purchase has committed successfully.
pub async fn record_first_character_part(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<Unlock>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let unlocked = record_first_character_part_in_tx(&mut tx, user_id).await?;
    tx.commit().await?;
    Ok(unlocked)
}

/// Transaction-aware purchase hook; call only after the purchase succeeded,
/// inside the transaction that commits the purchase.
pub async fn record_first_character_part_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<Vec<Unlock>, sqlx::Error> {
    unlock_matching(tx, user_id, "first_purchase", 1).await
}

async fn unlock_matching(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    kind: &str,
    value: i64,
) -> Result<Vec<Unlock>, sqlx::Error> {
    let eligible: Vec<Unlock> = sqlx::query_as(
        "SELECT id AS achievement_id, level, points FROM achievements
         WHERE enabled AND condition_type = $1 AND threshold <= $2
         ORDER BY sort_order, level",
    )
    .bind(kind)
    .bind(value)
    .fetch_all(&mut **tx)
    .await?;
    let mut new_unlocks = Vec::new();
    for item in eligible {
        let inserted = sqlx::query(
            "INSERT INTO achievement_unlocks (user_id, achievement_id, level)
             VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(&item.achievement_id)
        .bind(item.level)
        .execute(&mut **tx)
        .await?
        .rows_affected()
            == 1;
        if inserted {
            if item.points > 0 {
                sqlx::query(
                    "INSERT INTO point_ledger (user_id, delta, reason, source_type, source_id)
                     VALUES ($1, $2, $3, 'achievement', $4)",
                )
                .bind(user_id)
                .bind(item.points)
                .bind(format!(
                    "Achievement {} level {}",
                    item.achievement_id, item.level
                ))
                .bind(format!("{}:{}", item.achievement_id, item.level))
                .execute(&mut **tx)
                .await?;
            }
            new_unlocks.push(item);
        }
    }
    Ok(new_unlocks)
}
