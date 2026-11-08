//! F-19.3：每日／每週完成對話任務。週期固定 Asia/Taipei，領取與成就在同一交易。
use std::num::NonZeroU32;

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, patch, post},
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgConnection};
use uuid::Uuid;

use crate::{
    AppState, achievements,
    auth::{RequireAdmin, RequireStudent},
    error::ApiError,
    points::{self, Entry, Source},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/tasks", get(list))
        .route("/api/tasks/{id}/claim", post(claim))
        .route("/api/admin/tasks", get(admin_list))
        .route("/api/admin/tasks/{id}", patch(update))
}

#[derive(Clone, Debug, FromRow, Serialize, utoipa::ToSchema)]
pub(crate) struct Task {
    id: String,
    period: String,
    goal: i32,
    reward: i32,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct TaskView {
    id: String,
    period: String,
    goal: i32,
    reward: i32,
    period_start: NaiveDate,
    progress: i64,
    claimed: bool,
}

impl TaskView {
    fn new(task: Task, period_start: NaiveDate, progress: i64, claimed: bool) -> Self {
        Self {
            id: task.id,
            period: task.period,
            goal: task.goal,
            reward: task.reward,
            period_start,
            progress,
            claimed,
        }
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Claim {
    period_start: NaiveDate,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct TaskUpdate {
    goal: i32,
    reward: i32,
}

async fn period_start(conn: &mut PgConnection, period: &str) -> Result<NaiveDate, sqlx::Error> {
    sqlx::query_scalar("SELECT CASE WHEN $1 = 'weekly' THEN date_trunc('week', clock_timestamp() AT TIME ZONE 'Asia/Taipei')::date ELSE (clock_timestamp() AT TIME ZONE 'Asia/Taipei')::date END")
        .bind(period).fetch_one(conn).await
}

async fn progress(
    conn: &mut PgConnection,
    user_id: Uuid,
    task: &Task,
    start: NaiveDate,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM achievement_completion_events WHERE user_id = $1 AND completed_at >= ($2::date::timestamp AT TIME ZONE 'Asia/Taipei') AND completed_at < (($2::date + CASE WHEN $3 = 'weekly' THEN 7 ELSE 1 END)::timestamp AT TIME ZONE 'Asia/Taipei')")
        .bind(user_id).bind(start).bind(&task.period).fetch_one(conn).await
}

fn source_id(id: &str, start: NaiveDate) -> String {
    format!("{id}:{start}")
}

#[utoipa::path(get, path = "/api/tasks", tag = "tasks", summary = "我的任務", responses((status = 200, body = [TaskView])))]
pub(crate) async fn list(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
) -> Result<Json<Vec<TaskView>>, ApiError> {
    let mut conn = state.pool.acquire().await?;
    let tasks: Vec<Task> =
        sqlx::query_as("SELECT id, period, goal, reward FROM tasks ORDER BY period")
            .fetch_all(&mut *conn)
            .await?;
    let mut result = Vec::with_capacity(tasks.len());
    for task in tasks {
        let start = period_start(&mut conn, &task.period).await?;
        let progress = progress(&mut conn, cu.user.id, &task, start).await?;
        let claimed: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM point_ledger WHERE user_id=$1 AND source_type='task' AND source_id=$2)")
            .bind(cu.user.id).bind(source_id(&task.id, start)).fetch_one(&mut *conn).await?;
        result.push(TaskView::new(task, start, progress, claimed));
    }
    Ok(Json(result))
}

#[utoipa::path(post, path = "/api/tasks/{id}/claim", tag = "tasks", summary = "領取任務獎勵", params(("id" = String, Path)), request_body = Claim, responses((status = 200, body = TaskView), (status = 409, body = crate::error::ErrorBody)))]
pub(crate) async fn claim(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Claim>,
) -> Result<Json<TaskView>, ApiError> {
    let mut tx = state.pool.begin().await?;
    achievements::lock_catalog_for_event(&mut tx).await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR NO KEY UPDATE")
        .bind(cu.user.id)
        .execute(&mut *tx)
        .await?;
    let task: Task =
        sqlx::query_as("SELECT id, period, goal, reward FROM tasks WHERE id=$1 FOR SHARE")
            .bind(&id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(ApiError::not_found)?;
    let start = period_start(&mut tx, &task.period).await?;
    if body.period_start != start {
        return Err(ApiError::conflict("conflict", "任務週期已結束"));
    }
    let progress = progress(&mut tx, cu.user.id, &task, start).await?;
    if progress < i64::from(task.goal) {
        return Err(ApiError::conflict("conflict", "尚未完成任務"));
    }
    let source_id = source_id(&task.id, start);
    let points = NonZeroU32::new(task.reward as u32).ok_or_else(ApiError::internal)?;
    let entry = Entry::award(cu.user.id, points, "task_reward", Source::Task, &source_id);
    if !points::record(&mut tx, &entry).await? {
        return Err(ApiError::conflict("conflict", "任務獎勵已領取"));
    }
    achievements::record_first_task_reward_in_tx(&mut tx, cu.user.id).await?;
    tx.commit().await?;
    Ok(Json(TaskView::new(task, start, progress, true)))
}

#[utoipa::path(get, path = "/api/admin/tasks", tag = "tasks", summary = "任務設定（管理者）", responses((status = 200, body = [Task])))]
pub(crate) async fn admin_list(
    RequireAdmin(_): RequireAdmin,
    State(state): State<AppState>,
) -> Result<Json<Vec<Task>>, ApiError> {
    Ok(Json(
        sqlx::query_as("SELECT id, period, goal, reward FROM tasks ORDER BY period")
            .fetch_all(&state.pool)
            .await?,
    ))
}

#[utoipa::path(patch, path = "/api/admin/tasks/{id}", tag = "tasks", summary = "修改任務設定（管理者）", params(("id" = String, Path)), request_body = TaskUpdate, responses((status = 200, body = Task), (status = 400, body = crate::error::ErrorBody)))]
pub(crate) async fn update(
    RequireAdmin(cu): RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<TaskUpdate>,
) -> Result<Json<Task>, ApiError> {
    if body.goal <= 0 || body.reward <= 0 {
        return Err(ApiError::bad_request(
            "invalid_request",
            "任務目標及獎勵必須大於零",
        ));
    }
    let mut tx = state.pool.begin().await?;
    let old: Task =
        sqlx::query_as("SELECT id, period, goal, reward FROM tasks WHERE id=$1 FOR UPDATE")
            .bind(&id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(ApiError::not_found)?;
    let new: Task = sqlx::query_as(
        "UPDATE tasks SET goal=$2, reward=$3 WHERE id=$1 RETURNING id, period, goal, reward",
    )
    .bind(&id)
    .bind(body.goal)
    .bind(body.reward)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO audit_logs (actor_id, action, target_email, old_value, new_value) VALUES ($1, 'task_updated', $2, $3, $4)")
        .bind(cu.user.id).bind(&id).bind(sqlx::types::Json(&old)).bind(sqlx::types::Json(&new)).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(new))
}
