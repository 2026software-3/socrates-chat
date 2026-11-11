//! F-24.1-DB (#524): single-conversation radar persistence, independent of AI and HTTP.
//!
//! Callers must authorize the conversation, check its classroom lifecycle and obtain
//! completed topic dimensions before claiming work. No endpoint is exposed by this module.
use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool, types::Json};
use uuid::Uuid;

pub type Scores = BTreeMap<String, u8>;

#[derive(Debug, FromRow)]
pub struct RadarResult {
    pub conversation_id: Uuid,
    pub status: String,
    pub attempt: i32,
    pub scores: Option<Json<Scores>>,
    pub rules_version: Option<String>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

pub async fn get(pool: &PgPool, id: Uuid) -> Result<Option<RadarResult>, sqlx::Error> {
    sqlx::query_as("SELECT conversation_id, status, attempt, scores, rules_version, started_at, completed_at FROM conversation_radar_results WHERE conversation_id=$1")
        .bind(id).fetch_optional(pool).await
}

/// Claim an initial attempt or retry a failure. Concurrent requests can claim only once.
/// Missing/incomplete summaries, active conversations, pending and ready results return None.
pub async fn claim(pool: &PgPool, id: Uuid) -> Result<Option<i32>, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO conversation_radar_results (conversation_id)
         SELECT c.id FROM conversations c JOIN summaries s ON s.conversation_id=c.id
         WHERE c.id=$1 AND c.status='ended' AND s.status='ready'
         ON CONFLICT (conversation_id) DO UPDATE
         SET status='pending', attempt=conversation_radar_results.attempt+1,
             scores=NULL, rules_version=NULL, started_at=now(), completed_at=NULL
         WHERE conversation_radar_results.status='failed'
         RETURNING attempt",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// Complete only the current pending attempt. The database rejects partial/invalid scores.
/// Invalid writes leave the attempt pending for the caller to mark failed; summaries are untouched.
pub async fn complete(
    pool: &PgPool,
    id: Uuid,
    attempt: i32,
    scores: &Scores,
    rules_version: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE conversation_radar_results SET status='ready', scores=$3,
             rules_version=$4, completed_at=now()
         WHERE conversation_id=$1 AND attempt=$2 AND status='pending'",
    )
    .bind(id)
    .bind(attempt)
    .bind(Json(scores))
    .bind(rules_version)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn fail(pool: &PgPool, id: Uuid, attempt: i32) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE conversation_radar_results SET status='failed', completed_at=now()
         WHERE conversation_id=$1 AND attempt=$2 AND status='pending'",
    )
    .bind(id)
    .bind(attempt)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Startup-only recovery hook for future BE wiring; do not call while jobs are running.
pub async fn recover_orphaned(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("UPDATE conversation_radar_results SET status='failed', completed_at=now() WHERE status='pending'")
        .execute(pool).await?;
    Ok(result.rows_affected())
}
