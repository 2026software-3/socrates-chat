//! Achievement evaluation and idempotent unlocks (F-18.1, S-07.3).
use std::num::NonZeroU32;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::points::{self, Entry, Source};

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
    lock_catalog_for_event(tx).await?;
    lock_user(tx, user_id).await?;
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

    let count = completion_count(tx, user_id).await?;
    let streak = longest_streak(tx, user_id).await?;
    let mut unlocked = Vec::new();
    unlocked.extend(unlock_matching(tx, user_id, "completed_conversations", count, None).await?);
    unlocked.extend(unlock_matching(tx, user_id, "consecutive_days", streak, None).await?);
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
    lock_catalog_for_event(tx).await?;
    unlock_matching(
        tx,
        user_id,
        "dimension_score",
        i64::from(*scores.iter().max().unwrap()),
        None,
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

/// Transaction-aware task hook. If the caller locks the user row, it must lock
/// the catalog for the event first.
pub async fn record_first_task_reward_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<Vec<Unlock>, sqlx::Error> {
    lock_catalog_for_event(tx).await?;
    unlock_matching(tx, user_id, "first_task_claim", 1, None).await
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
/// inside the transaction that commits the purchase. If the caller locks the
/// user row, it must lock the catalog for the event first.
pub async fn record_first_character_part_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<Vec<Unlock>, sqlx::Error> {
    lock_catalog_for_event(tx).await?;
    unlock_matching(tx, user_id, "first_purchase", 1, None).await
}

/// Events can run concurrently, but a catalog change and its backfill must
/// see either all or none of each event transaction. Lock the catalog before
/// taking any user-row lock so events and backfills use the same lock order.
pub(crate) async fn lock_catalog_for_event(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<(), sqlx::Error> {
    sqlx::query("LOCK TABLE achievements IN ROW EXCLUSIVE MODE")
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub(crate) async fn lock_catalog_for_update(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<(), sqlx::Error> {
    sqlx::query("LOCK TABLE achievements IN SHARE ROW EXCLUSIVE MODE")
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Serialize a student's progress evaluation so concurrent completions
/// cannot both observe a stale count and skip a threshold permanently.
async fn lock_user(tx: &mut Transaction<'_, Postgres>, user_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT id FROM users WHERE id = $1 FOR NO KEY UPDATE")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn completion_count(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM achievement_completion_events WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(&mut **tx)
        .await
}

/// Longest run of adjacent Asia/Taipei calendar days with at least one completion.
async fn longest_streak(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
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
    .await
}

/// Unlock every enabled, unlocked-not-yet level of `kind` whose threshold `value` reaches.
/// `only` restricts the evaluation to one achievement (used by retroactive runs).
async fn unlock_matching(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    kind: &str,
    value: i64,
    only: Option<&str>,
) -> Result<Vec<Unlock>, sqlx::Error> {
    let eligible: Vec<Unlock> = sqlx::query_as(
        "SELECT id AS achievement_id, level, points FROM achievements
         WHERE enabled AND condition_type = $1 AND threshold <= $2
           AND ($4::text IS NULL OR id = $4)
           AND NOT EXISTS (
               SELECT 1 FROM achievement_unlocks u
               WHERE u.user_id = $3 AND u.achievement_id = achievements.id
                 AND u.level = achievements.level)
         ORDER BY sort_order, level",
    )
    .bind(kind)
    .bind(value)
    .bind(user_id)
    .bind(only)
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
            if let Some(points) = NonZeroU32::new(item.points as u32) {
                let reason = format!("Achievement {} level {}", item.achievement_id, item.level);
                let source_id = format!("{}:{}", item.achievement_id, item.level);
                let entry = Entry::award(user_id, points, &reason, Source::Achievement, &source_id);
                points::record(tx, &entry).await?;
            }
            new_unlocks.push(item);
        }
    }
    Ok(new_unlocks)
}

/// One-time retroactive evaluation of a single achievement for every student (S-07.3 5.4).
///
/// Call it in the transaction that created, lowered the threshold of, or re-enabled the
/// achievement. Students are locked one at a time in id order, so it is safe next to live
/// completions, and a rerun finds nothing left to unlock.
///
/// `dimension_score` has no persisted score source yet (radar scores are not stored), so it
/// is skipped here; its first retroactive run belongs to the slice that stores the scores.
pub async fn backfill_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    achievement_id: &str,
) -> Result<Vec<Unlock>, sqlx::Error> {
    lock_catalog_for_update(tx).await?;
    let kind: Option<String> =
        sqlx::query_scalar("SELECT condition_type FROM achievements WHERE id = $1 LIMIT 1")
            .bind(achievement_id)
            .fetch_optional(&mut **tx)
            .await?;
    let Some(kind) = kind else {
        return Ok(vec![]);
    };
    let candidates_sql = match kind.as_str() {
        "completed_conversations" | "consecutive_days" => {
            "SELECT DISTINCT user_id FROM achievement_completion_events ORDER BY user_id"
        }
        "first_task_claim" => {
            "SELECT DISTINCT user_id FROM point_ledger WHERE source_type = 'task' ORDER BY user_id"
        }
        "first_purchase" => {
            "SELECT DISTINCT user_id FROM point_ledger WHERE source_type = 'purchase' ORDER BY user_id"
        }
        _ => return Ok(vec![]),
    };
    let candidates: Vec<Uuid> = sqlx::query_scalar(candidates_sql)
        .fetch_all(&mut **tx)
        .await?;
    let mut unlocked = Vec::new();
    for user_id in candidates {
        lock_user(tx, user_id).await?;
        let value = match kind.as_str() {
            "completed_conversations" => completion_count(tx, user_id).await?,
            "consecutive_days" => longest_streak(tx, user_id).await?,
            _ => 1,
        };
        unlocked.extend(unlock_matching(tx, user_id, &kind, value, Some(achievement_id)).await?);
    }
    Ok(unlocked)
}

/// Pool variant of [`backfill_in_tx`].
pub async fn backfill(pool: &PgPool, achievement_id: &str) -> Result<Vec<Unlock>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let unlocked = backfill_in_tx(&mut tx, achievement_id).await?;
    tx.commit().await?;
    Ok(unlocked)
}
