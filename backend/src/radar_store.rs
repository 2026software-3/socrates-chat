//! F-24.1-DB (#524): single-conversation radar persistence, independent of AI and HTTP.
//!
//! Callers must authorize the conversation, check its classroom lifecycle and obtain
//! completed topic dimensions before claiming work. No endpoint is exposed by this module.
use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool, types::Json};
use uuid::Uuid;

pub type Scores = BTreeMap<String, u8>;

/// Which dimensions a result must score and the highest score. The database only guards the
/// shape of `scores`; this is the one place that knows the scale, so a topic with other
/// dimension ids (F-03.4) or another scale needs no schema change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rubric {
    dimension_ids: Vec<String>,
    max_score: u8,
}

/// Why a score set was refused. Carries dimension ids only, never conversation content.
#[derive(Debug, PartialEq, Eq)]
pub enum InvalidScores {
    Missing(String),
    Unknown(String),
    OutOfRange(String, u8),
}

#[derive(Debug)]
pub enum CompleteError {
    Invalid(InvalidScores),
    Db(sqlx::Error),
}

impl From<sqlx::Error> for CompleteError {
    fn from(error: sqlx::Error) -> Self {
        Self::Db(error)
    }
}

impl Rubric {
    pub fn new(dimension_ids: Vec<String>, max_score: u8) -> Self {
        Self {
            dimension_ids,
            max_score,
        }
    }

    /// S-04.2 / S-04.6: six dimensions `d1`–`d6`, integer scores 0 to 5.
    pub fn standard() -> Self {
        Self::new((1..=6).map(|n| format!("d{n}")).collect(), 5)
    }

    /// Every dimension must be present and within the scale; no extra keys.
    pub fn validate(&self, scores: &Scores) -> Result<(), InvalidScores> {
        let expected: BTreeSet<&str> = self.dimension_ids.iter().map(String::as_str).collect();
        if let Some(missing) = expected.iter().find(|id| !scores.contains_key(**id)) {
            return Err(InvalidScores::Missing((*missing).to_owned()));
        }
        for (id, score) in scores {
            if !expected.contains(id.as_str()) {
                return Err(InvalidScores::Unknown(id.clone()));
            }
            if *score > self.max_score {
                return Err(InvalidScores::OutOfRange(id.clone(), *score));
            }
        }
        Ok(())
    }
}

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

/// Claim an initial attempt, retry a failure, or take over a pending attempt that has been
/// running longer than `stale_secs` (its worker died or hung). Takeover opens a new attempt, so
/// the old worker's later `complete`/`fail` is ignored. Concurrent requests can claim only once.
/// Missing/incomplete summaries, active conversations, fresh pending and ready results return None.
pub async fn claim(pool: &PgPool, id: Uuid, stale_secs: f64) -> Result<Option<i32>, sqlx::Error> {
    start(pool, id, stale_secs, false).await
}

/// Like `claim`, but also restarts a `ready` result (for example after a scoring-rules change).
/// The previous scores are discarded when the new attempt starts, so the conversation has no
/// radar until it completes. Who may trigger this and when is not decided by the specs yet;
/// callers own that authorization.
pub async fn recompute(
    pool: &PgPool,
    id: Uuid,
    stale_secs: f64,
) -> Result<Option<i32>, sqlx::Error> {
    start(pool, id, stale_secs, true).await
}

async fn start(
    pool: &PgPool,
    id: Uuid,
    stale_secs: f64,
    include_ready: bool,
) -> Result<Option<i32>, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO conversation_radar_results (conversation_id)
         SELECT c.id FROM conversations c JOIN summaries s ON s.conversation_id=c.id
         WHERE c.id=$1 AND c.status='ended' AND s.status='ready'
         ON CONFLICT (conversation_id) DO UPDATE
         SET status='pending', attempt=conversation_radar_results.attempt+1,
             scores=NULL, rules_version=NULL, started_at=now(), completed_at=NULL
         WHERE conversation_radar_results.status='failed'
            OR ($3 AND conversation_radar_results.status='ready')
            OR (conversation_radar_results.status='pending'
                AND conversation_radar_results.started_at < now() - make_interval(secs => $2))
         RETURNING attempt",
    )
    .bind(id)
    .bind(stale_secs)
    .bind(include_ready)
    .fetch_optional(pool)
    .await
}

/// Complete only the current pending attempt, and only while the conversation is still ended
/// with a ready summary (the same conditions `claim` requires). Scores are validated against
/// `rubric` first; invalid scores return an error and leave the attempt pending for the caller
/// to mark failed. `Ok(false)` means the attempt is no longer current or the summary is not
/// ready any more; nothing was written. Summaries are never touched.
pub async fn complete(
    pool: &PgPool,
    id: Uuid,
    attempt: i32,
    scores: &Scores,
    rubric: &Rubric,
    rules_version: &str,
) -> Result<bool, CompleteError> {
    rubric.validate(scores).map_err(CompleteError::Invalid)?;
    let result = sqlx::query(
        "UPDATE conversation_radar_results r SET status='ready', scores=$3,
             rules_version=$4, completed_at=now()
         WHERE r.conversation_id=$1 AND r.attempt=$2 AND r.status='pending'
           AND EXISTS (SELECT 1 FROM conversations c JOIN summaries s ON s.conversation_id=c.id
                       WHERE c.id=r.conversation_id AND c.status='ended' AND s.status='ready')",
    )
    .bind(id)
    .bind(attempt)
    .bind(Json(scores))
    .bind(rules_version)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Mark the current pending attempt failed. Not conditioned on the summary: `failed` is always
/// a valid state, and refusing it would only leave a dead attempt looking in progress.
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

/// Startup recovery hook for future BE wiring: marks only pending attempts older than
/// `stale_secs` as failed, so fresh in-flight work (e.g. another instance during a rolling
/// restart) is left alone. Use the same threshold as `claim`.
pub async fn recover_orphaned(pool: &PgPool, stale_secs: f64) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE conversation_radar_results SET status='failed', completed_at=now()
         WHERE status='pending' AND started_at < now() - make_interval(secs => $1)",
    )
    .bind(stale_secs)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
