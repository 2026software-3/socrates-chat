//! 單場雷達圖的計算流程與學生 API（F-24.1-BE，#567；規格見 S-04.2、S-04.3、S-04.6）。
//!
//! 總結成功時在同一個交易內登記計算、隨後在背景執行；學生輪詢 `GET .../radar` 看狀態，失敗、尚未計算（例如題目維度
//! 後來才完成）或計算卡住時可用 `POST .../radar/retry` 重新計算。計算失敗不影響總結。
//! 資料存取在 `radar_store`，評分提示與結果驗證在 `radar_scoring`。

use std::collections::BTreeMap;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{Acquire, FromRow, PgExecutor, Postgres, Transaction, types::Json as SqlJson};
use tracing::Instrument;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::RequireStudent,
    chat::owned_conversation,
    diagnostics::{self, AiActivity},
    error::{ApiError, db_error_class},
    radar_scoring::{self, PROMPT_VERSION},
    radar_store::{self, CompleteError, Scores},
    topic_dimensions::{Dimension, Localized},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/conversations/{id}/radar", get(read))
        .route("/api/conversations/{id}/radar/retry", post(retry))
}

/// 單場雷達圖的狀態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RadarStatus {
    /// 對話尚未結束，或總結還沒成功：沒有可計算的內容
    SummaryNotReady,
    /// 題目已不存在，無法計算
    Unavailable,
    /// 題目的六個維度尚未完成；完成後可手動觸發
    DimensionsPending,
    /// 可以計算但還沒開始
    NotStarted,
    Pending,
    Failed,
    Ready,
}

/// 題目的一個維度：給學生看的名稱與白話說明（三語），不含評分準則。
#[derive(Serialize, ToSchema)]
pub(crate) struct RadarDimension {
    id: String,
    name: Localized,
    description: Localized,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct RadarView {
    status: RadarStatus,
    /// 現在是否可以呼叫 `radar/retry`（失敗、尚未開始，或計算已超過逾時上限）
    retryable: bool,
    /// 以維度識別碼為 key 的 0 到 5 整數分；只有 `ready` 時有值，且六個維度齊全
    scores: Option<BTreeMap<String, u8>>,
    /// 題目的六個維度；題目維度尚未完成或題目已不存在時為 null
    dimensions: Option<Vec<RadarDimension>>,
    /// 評分提示版本；只有 `ready` 時有值
    rules_version: Option<String>,
    /// 只有 `ready` 時有值
    completed_at: Option<DateTime<Utc>>,
}

#[derive(FromRow)]
struct Row {
    owner: Uuid,
    classroom: Option<Uuid>,
    title: String,
    guiding_questions: Vec<String>,
    has_topic: bool,
    summary_ready: bool,
    dimensions: Option<SqlJson<[Dimension; 6]>>,
    dimensions_language: Option<String>,
    radar_status: Option<String>,
    scores: Option<SqlJson<Scores>>,
    rules_version: Option<String>,
    completed_at: Option<DateTime<Utc>>,
    stale: bool,
    /// 所屬教室已封存：唯讀，不能開始計算（S-14）
    archived: bool,
}

/// 給學生看的維度：名稱與白話說明，不含評分準則。
fn public_dimensions(dimensions: [Dimension; 6]) -> Vec<RadarDimension> {
    dimensions
        .into_iter()
        .map(|dimension| RadarDimension {
            id: dimension.id,
            name: dimension.name,
            description: dimension.description,
        })
        .collect()
}

/// 一次計算需要的輸入：只有這場對話自己的題目文字與題目維度。
struct Job {
    owner: Uuid,
    classroom: Option<Uuid>,
    title: String,
    guiding_questions: Vec<String>,
    dimensions: Vec<radar_scoring::Dimension>,
}

impl Row {
    fn status(&self) -> RadarStatus {
        if !self.summary_ready {
            RadarStatus::SummaryNotReady
        } else if !self.has_topic {
            RadarStatus::Unavailable
        } else if self.dimensions.is_none() {
            RadarStatus::DimensionsPending
        } else {
            match self.radar_status.as_deref() {
                None => RadarStatus::NotStarted,
                Some("ready") => RadarStatus::Ready,
                Some("failed") => RadarStatus::Failed,
                Some(_) => RadarStatus::Pending,
            }
        }
    }

    fn view(self) -> RadarView {
        let status = self.status();
        let ready = status == RadarStatus::Ready;
        RadarView {
            status,
            // 封存教室的重試一定回 409 classroom_archived，所以不讓畫面顯示重試
            retryable: !self.archived
                && (matches!(status, RadarStatus::NotStarted | RadarStatus::Failed)
                    || (status == RadarStatus::Pending && self.stale)),
            scores: self.scores.filter(|_| ready).map(|scores| scores.0),
            dimensions: self
                .dimensions
                .map(|dimensions| public_dimensions(dimensions.0)),
            rules_version: self.rules_version.filter(|_| ready),
            completed_at: self.completed_at.filter(|_| ready),
        }
    }

    /// 剛取得新一代計算時的回應：計算中、不能重試、沒有分數。
    /// 直接從已讀到的資料組出來，不必在提交後再查一次資料庫。
    fn pending_view(&self) -> RadarView {
        RadarView {
            status: RadarStatus::Pending,
            retryable: false,
            scores: None,
            dimensions: self
                .dimensions
                .as_ref()
                .map(|dimensions| public_dimensions(dimensions.0.clone())),
            rules_version: None,
            completed_at: None,
        }
    }

    /// 評分用的維度以評分準則的語言呈現（準則只有一種語言，F-03.4）。題目沒有維度時為 `None`。
    fn job(self) -> Option<Job> {
        let language = self.dimensions_language.unwrap_or_default();
        let dimensions = self
            .dimensions?
            .0
            .into_iter()
            .map(|dimension| radar_scoring::Dimension {
                name: dimension.name.get(&language).to_owned(),
                description: dimension.description.get(&language).to_owned(),
                id: dimension.id,
                rubric: dimension.rubric,
            })
            .collect();
        Some(Job {
            owner: self.owner,
            classroom: self.classroom,
            title: self.title,
            guiding_questions: self.guiding_questions,
            dimensions,
        })
    }
}

async fn fetch<'e>(
    conn: impl PgExecutor<'e>,
    id: Uuid,
    stale_secs: f64,
) -> Result<Option<Row>, sqlx::Error> {
    sqlx::query_as(
        "SELECT c.user_id AS owner, c.classroom_id AS classroom, c.title, COALESCE(t.guiding_questions, '{}') AS guiding_questions,
                t.id IS NOT NULL AS has_topic,
                COALESCE(s.status = 'ready', false) AS summary_ready,
                t.dimensions, t.dimensions_language,
                r.status AS radar_status, r.scores, r.rules_version, r.completed_at,
                COALESCE(r.status = 'pending'
                         AND r.started_at < now() - make_interval(secs => $2), false) AS stale,
                COALESCE(cl.is_archived, false) AS archived
         FROM conversations c
         LEFT JOIN classrooms cl ON cl.id = c.classroom_id
         LEFT JOIN summaries s ON s.conversation_id = c.id
         LEFT JOIN topics t ON t.id = c.topic_id
         LEFT JOIN conversation_radar_results r ON r.conversation_id = c.id
         WHERE c.id = $1",
    )
    .bind(id)
    .bind(stale_secs)
    .fetch_optional(conn)
    .await
}

async fn view(state: &AppState, id: Uuid) -> Result<RadarView, ApiError> {
    Ok(fetch(&state.pool, id, state.config.generation_stale_secs())
        .await?
        .ok_or_else(ApiError::not_found)?
        .view())
}

/// 已登記、尚未執行的一次計算。
pub(crate) struct Claimed {
    attempt: i32,
    job: Job,
}

/// 在總結變成 ready 的同一個交易內登記第一次計算：題目有維度才登記，沒有就不做
/// （維度完成後學生可手動觸發）。
///
/// 用 savepoint 包起來且不回傳錯誤：雷達圖的任何問題都不能讓總結交易失敗。
pub(crate) async fn claim_after_summary(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    stale_secs: f64,
) -> Option<Claimed> {
    let claimed = async {
        let mut savepoint = tx.begin().await?;
        let Some(job) = fetch(&mut *savepoint, id, stale_secs)
            .await?
            .and_then(Row::job)
        else {
            return Ok(None);
        };
        let attempt = radar_store::claim(&mut *savepoint, id, stale_secs).await?;
        savepoint.commit().await?;
        Ok::<_, sqlx::Error>(attempt.map(|attempt| Claimed { attempt, job }))
    };
    match claimed.await {
        Ok(claimed) => claimed,
        Err(e) => {
            tracing::error!(class = %db_error_class(&e), "radar scoring could not start");
            None
        }
    }
}

/// 在背景執行已登記的計算。
pub(crate) fn spawn_claimed(state: AppState, id: Uuid, claimed: Claimed) {
    tokio::spawn(diagnostics::inherit_request(
        async move { run_claimed(&state, id, claimed.attempt, claimed.job).await }
            .in_current_span(),
    ));
}

/// 執行已取得的那一代計算；記帳失敗時盡力標記為失敗，避免停在 pending。
async fn run_claimed(state: &AppState, id: Uuid, attempt: i32, job: Job) {
    if let Err(e) = run(state, id, attempt, &job).await {
        // 不記錄資料庫錯誤訊息：約束錯誤可能帶出資料列內容
        tracing::error!(class = %db_error_class(&e), "radar scoring bookkeeping failed");
        let _ = radar_store::fail(&state.pool, id, attempt).await;
    }
}

async fn run(state: &AppState, id: Uuid, attempt: i32, job: &Job) -> Result<(), sqlx::Error> {
    let turns = crate::summary::load_messages_for(state, id).await?;
    let rubric = radar_scoring::rubric_for(&job.dimensions);
    let request =
        radar_scoring::build_request(&job.title, &job.guiding_questions, &job.dimensions, &turns);
    // 逾時、失敗或結果無效時自動重試 1 次（S-03.5）；仍失敗就標記 failed
    let mut scores = None;
    // 教室內的對話只用教室建立者的 key（F-40.5）
    let route = state.ai_for(job.classroom).await;
    for retry in 0..2 {
        let Ok(route) = &route else { break };
        let scored = AiActivity::new("radar_scoring", Some(job.owner), job.classroom)
            .statuses(202, 202)
            .run(
                state,
                Some(&route.model),
                retry,
                radar_scoring::score(
                    route.ai.as_ref(),
                    request.clone(),
                    &rubric,
                    state.config.ai_total_timeout,
                ),
            )
            .await;
        if let Ok(valid) = scored {
            scores = Some(valid);
            break;
        }
    }
    let Some(scores) = scores else {
        radar_store::fail(&state.pool, id, attempt).await?;
        return Ok(());
    };
    // 分數與成就判定在同一個交易內（S-07.3：雷達圖分數寫入後判定）
    let mut tx = state.pool.begin().await?;
    match radar_store::complete(&mut *tx, id, attempt, &scores, &rubric, PROMPT_VERSION).await {
        Ok(true) => {
            let values: Vec<i32> = scores.values().map(|score| i32::from(*score)).collect();
            crate::achievements::record_dimension_score_in_tx(&mut tx, job.owner, &values).await?;
            tx.commit().await?;
        }
        // 這一代已被新的嘗試取代，或總結已不是 ready：不寫入
        Ok(false) => {}
        Err(CompleteError::Db(e)) => return Err(e),
        Err(CompleteError::Invalid(_)) => {
            drop(tx);
            radar_store::fail(&state.pool, id, attempt).await?;
        }
    }
    Ok(())
}

#[utoipa::path(
    get,
    path = "/api/conversations/{id}/radar",
    tag = "conversations",
    summary = "讀取單場雷達圖（pending 時請輪詢）",
    params(("id" = Uuid, Path, description = "對話 ID")),
    responses((status = 200, description = "單場雷達圖的狀態與結果", body = RadarView))
)]
pub(crate) async fn read(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RadarView>, ApiError> {
    owned_conversation(&state, cu.user.id, id).await?;
    Ok(Json(view(&state, id).await?))
}

#[utoipa::path(
    post,
    path = "/api/conversations/{id}/radar/retry",
    tag = "conversations",
    summary = "計算或重新計算單場雷達圖",
    params(("id" = Uuid, Path, description = "對話 ID")),
    responses((status = 202, description = "已開始計算", body = RadarView), (status = 409, description = "classroom_archived / dimensions_pending / radar_not_retryable", body = crate::error::ErrorBody))
)]
pub(crate) async fn retry(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<RadarView>), ApiError> {
    let stale = state.config.generation_stale_secs();
    let not_retryable = || ApiError::conflict("radar_not_retryable", "目前無法計算雷達圖");
    // 封存檢查與開始計算在同一個交易內
    let mut tx = state.pool.begin().await?;
    crate::classrooms::writable_conversation(&mut tx, cu.user.id, id).await?;
    let row = fetch(&mut *tx, id, stale)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if row.status() == RadarStatus::DimensionsPending {
        return Err(ApiError::conflict(
            "dimensions_pending",
            "題目的維度尚未完成",
        ));
    }
    let pending = row.pending_view();
    let job = row.job().ok_or_else(not_retryable)?;
    // 只有失敗、尚未開始，或產生中卻已超過逾時上限才會取得新的一代；
    // 舊的背景任務之後寫入會因代數不符而被忽略
    let attempt = radar_store::claim(&mut *tx, id, stale)
        .await?
        .ok_or_else(not_retryable)?;
    // 提交與執行交給背景任務：請求在提交當下或之後被取消（斷線、請求逾時）時，
    // 這一代仍會提交並執行，不會停在 pending 卻沒有工作在跑。
    // 回應用提交前讀到的資料組出，所以很快的 AI 也不會讓 202 的內容變成 ready。
    let (committed, outcome) = tokio::sync::oneshot::channel();
    tokio::spawn(diagnostics::inherit_request(
        async move {
            match tx.commit().await {
                Ok(()) => {
                    let _ = committed.send(Ok(()));
                    run_claimed(&state, id, attempt, job).await;
                }
                Err(e) => {
                    let _ = committed.send(Err(e));
                }
            }
        }
        .in_current_span(),
    ));
    outcome.await.map_err(|_| ApiError::internal())??;
    Ok((StatusCode::ACCEPTED, Json(pending)))
}
