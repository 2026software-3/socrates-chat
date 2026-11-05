//! Achievement evaluation and idempotent unlocks (F-18.1, S-07.3).
use std::num::NonZeroU32;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::points::{self, Entry, Source};

#[derive(Debug, Clone, Serialize, sqlx::FromRow, PartialEq, Eq)]
pub struct Unlock {
    pub achievement_id: String,
    pub level: i32,
    pub points: i32,
}

/// Record a summarized conversation exactly once, then award every newly met
/// completion-count or Taipei-streak level with its points. Runs in the caller's
/// transaction so completion and awards commit or roll back together.
pub async fn record_completed_conversation(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    conversation_id: Uuid,
    completed_at: DateTime<Utc>,
) -> Result<Vec<Unlock>, sqlx::Error> {
    // Serialize a student's progress evaluation so concurrent completions
    // cannot both observe a stale count and skip a threshold permanently.
    sqlx::query("SELECT id FROM users WHERE id = $1 FOR NO KEY UPDATE")
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

async fn unlock_matching(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    kind: &str,
    value: i64,
) -> Result<Vec<Unlock>, sqlx::Error> {
    let eligible: Vec<Unlock> = sqlx::query_as(
        "SELECT id AS achievement_id, level, points FROM achievements
         WHERE enabled AND condition_type = $1 AND threshold <= $2
           AND NOT EXISTS (
               SELECT 1 FROM achievement_unlocks u
               WHERE u.user_id = $3 AND u.achievement_id = achievements.id
                 AND u.level = achievements.level)
         ORDER BY sort_order, level",
    )
    .bind(kind)
    .bind(value)
    .bind(user_id)
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
