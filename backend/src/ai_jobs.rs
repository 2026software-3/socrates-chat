//! `GET /api/ai-jobs/active`：目前使用者進行中與剛結束的 AI 工作（S-20 §3.3）。
//!
//! 前端的全域進度顯示用它在重新整理後還原進行中的工作。這裡只彙總各功能自己的狀態表
//! （總結、雷達、論證樹、AI 回覆中、總結翻譯），不另存資料。
//! 回應只帶識別碼與狀態，不帶題目、訊息或總結內容；只回傳發起者自己的工作。

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{AppState, auth::AnyCurrentUser, error::ApiError};

/// 「剛結束」的時間窗（秒）：結束後在這段時間內仍列出，讓使用者看到完成或失敗。暫定 10 分鐘（S-20 §6）。
const RECENT_SECS: i64 = 600;
/// AI 回覆「產生中」標記超過這段時間視為過期（服務重啟留下的殘值），不列出。
const REPLY_STALE_SECS: i64 = 900;
const LIMIT: i64 = 50;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/ai-jobs/active", get(active))
        .route("/api/ai-jobs/{id}", get(get_job))
}

/// 工作的來源識別碼，只有識別碼；前端用它連回來源頁。
#[derive(Serialize, Deserialize, Default, utoipa::ToSchema)]
pub(crate) struct JobScope {
    #[serde(skip_serializing_if = "Option::is_none")]
    conversation_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    activity_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    topic_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    classroom_id: Option<Uuid>,
    /// 成就識別碼是文字（不是 UUID）。
    #[serde(skip_serializing_if = "Option::is_none")]
    achievement_id: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct AiJob {
    /// 對話相關的工作是 `{kind}:{conversation_id}`（同一個對話同一種工作只會有一筆）；
    /// 背景工作（`ai_jobs`）是工作的 UUID，可用 `GET /api/ai-jobs/{id}` 查詢。
    id: String,
    /// `reply`、`summary`、`radar`、`argument_tree`、`summary_translation`、`regroup`、`summary_translation_view`（教師按需翻譯）、`topic_dimensions`、`achievement_translation`。
    kind: String,
    /// `pending`、`ready`、`failed`。
    status: String,
    /// 前端 i18n 鍵，由前端翻譯。
    label_key: String,
    scope: JobScope,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    /// 失敗後可由使用者重試。
    retryable: bool,
}

/// 背景工作登記成功的回應。
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct JobAccepted {
    /// 用 `GET /api/ai-jobs/{id}` 查詢。
    job_id: Uuid,
    status: &'static str,
}

impl JobAccepted {
    pub(crate) fn pending(job_id: Uuid) -> Self {
        JobAccepted {
            job_id,
            status: "pending",
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ActiveJobs {
    jobs: Vec<AiJob>,
}

#[derive(FromRow)]
struct Row {
    kind: String,
    status: String,
    conversation_id: Uuid,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    retryable: bool,
}

impl From<Row> for AiJob {
    fn from(r: Row) -> Self {
        AiJob {
            id: format!("{}:{}", r.kind, r.conversation_id),
            label_key: format!("jobs.{}", r.kind),
            kind: r.kind,
            status: r.status,
            scope: JobScope {
                conversation_id: Some(r.conversation_id),
                ..JobScope::default()
            },
            started_at: r.started_at,
            finished_at: r.finished_at,
            retryable: r.retryable,
        }
    }
}

/// 目前登入者進行中，以及最近 10 分鐘內剛結束的 AI 工作。
#[utoipa::path(
    get,
    path = "/api/ai-jobs/active",
    tag = "conversations",
    summary = "進行中與剛結束的 AI 工作",
    description = "彙總自己的對話回覆中、總結、單場雷達、論證樹與總結翻譯的狀態，最新的在前，最多 50 筆。\
                   只含識別碼與狀態，不含任何內容。前端用它在重新整理後還原進度顯示（S-20）。",
    responses((status = 200, description = "工作清單", body = ActiveJobs), crate::error::CommonErrors)
)]
pub(crate) async fn active(
    AnyCurrentUser(cu): AnyCurrentUser,
    State(state): State<AppState>,
) -> Result<Json<ActiveJobs>, ApiError> {
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT kind, status, conversation_id, started_at, finished_at, retryable FROM (
            SELECT 'summary' AS kind, s.status, c.id AS conversation_id,
                   s.started_at, s.completed_at AS finished_at, (s.status = 'failed') AS retryable,
                   COALESCE(s.completed_at, s.started_at) AS touched
              FROM summaries s JOIN conversations c ON c.id = s.conversation_id
             WHERE c.user_id = $1
            UNION ALL
            SELECT 'radar', r.status, c.id,
                   r.started_at, r.completed_at, (r.status = 'failed'),
                   COALESCE(r.completed_at, r.started_at)
              FROM conversation_radar_results r JOIN conversations c ON c.id = r.conversation_id
             WHERE c.user_id = $1
            UNION ALL
            SELECT 'argument_tree', t.status, c.id,
                   t.updated_at, t.completed_at, (t.status = 'failed'),
                   COALESCE(t.completed_at, t.updated_at)
              FROM conversation_argument_trees t JOIN conversations c ON c.id = t.conversation_id
             WHERE c.user_id = $1
            UNION ALL
            SELECT 'reply', 'pending', c.id,
                   c.generating_since, NULL, false, c.generating_since
              FROM conversations c
             WHERE c.user_id = $1
               AND c.generating_since > now() - make_interval(secs => $4)
            UNION ALL
            SELECT 'summary_translation', 'pending', c.id,
                   s.completed_at, NULL, false, s.completed_at
              FROM summaries s JOIN conversations c ON c.id = s.conversation_id
             WHERE c.user_id = $1 AND s.status = 'ready' AND c.language <> $2
               AND NOT EXISTS (SELECT 1 FROM summary_translations t
                                WHERE t.conversation_id = s.conversation_id AND t.language = $2)
         ) j
         WHERE status = 'pending' OR touched > now() - make_interval(secs => $3)
         ORDER BY started_at DESC, kind, conversation_id
         LIMIT $5",
    )
    .bind(cu.user.id)
    .bind(&cu.user.language)
    .bind(RECENT_SECS as f64)
    .bind(REPLY_STALE_SECS as f64)
    .bind(LIMIT)
    .fetch_all(&state.pool)
    .await?;
    let background: Vec<BackgroundRow> = sqlx::query_as(
        "SELECT id, kind, status, scope, created_at, finished_at, error_code
           FROM ai_jobs
          WHERE owner_user_id = $1
            AND (status IN ('pending', 'running')
                 OR finished_at > now() - make_interval(secs => $2))
          ORDER BY created_at DESC
          LIMIT $3",
    )
    .bind(cu.user.id)
    .bind(RECENT_SECS as f64)
    .bind(LIMIT)
    .fetch_all(&state.pool)
    .await?;
    let mut jobs: Vec<AiJob> = rows.into_iter().map(AiJob::from).collect();
    jobs.extend(background.into_iter().map(|r| background_job(r).0));
    jobs.sort_by_key(|j| std::cmp::Reverse(j.started_at));
    jobs.truncate(LIMIT as usize);
    Ok(Json(ActiveJobs { jobs }))
}

#[derive(FromRow)]
struct BackgroundRow {
    id: Uuid,
    kind: String,
    status: String,
    scope: sqlx::types::Json<JobScope>,
    created_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    error_code: Option<String>,
}

/// 背景工作（`ai_jobs`）轉成對外的工作與錯誤代碼；`running` 對外一律是 `pending`。
fn background_job(r: BackgroundRow) -> (AiJob, Option<String>) {
    let status = if r.status == "running" {
        "pending".to_string()
    } else {
        r.status
    };
    (
        AiJob {
            id: r.id.to_string(),
            label_key: format!("jobs.{}", r.kind),
            kind: r.kind,
            retryable: status == "failed",
            status,
            scope: r.scope.0,
            started_at: r.created_at,
            finished_at: r.finished_at,
        },
        r.error_code,
    )
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct AiJobDetail {
    #[serde(flatten)]
    job: AiJob,
    /// 失敗時的固定錯誤代碼（例如 `ai_unavailable`、`interrupted`），不含任何診斷內容。
    error_code: Option<String>,
}

/// 查詢自己的一個背景工作。
#[utoipa::path(
    get,
    path = "/api/ai-jobs/{id}",
    tag = "conversations",
    summary = "查詢一個背景 AI 工作",
    description = "只有發起者能查；他人的與不存在的工作一律 404。回傳工作狀態與固定錯誤代碼，不含任何內容（S-20）。",
    params(("id" = Uuid, Path, description = "工作 ID")),
    responses((status = 200, description = "工作", body = AiJobDetail), (status = 404, description = "not_found", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn get_job(
    AnyCurrentUser(cu): AnyCurrentUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<AiJobDetail>, ApiError> {
    let row: BackgroundRow = sqlx::query_as(
        "SELECT id, kind, status, scope, created_at, finished_at, error_code
           FROM ai_jobs WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(id)
    .bind(cu.user.id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(ApiError::not_found)?;
    let (job, error_code) = background_job(row);
    Ok(Json(AiJobDetail { job, error_code }))
}
