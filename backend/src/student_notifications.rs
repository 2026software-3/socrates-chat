//! F-30.6：只儲存通知類型和時間。更正學號的流程可在同一個交易中建立通知。

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::{AppState, auth::RequireStudent, error::ApiError};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/notifications", get(list_unread))
        .route("/api/notifications/{id}/read", post(mark_read))
}

#[derive(Serialize, Deserialize, utoipa::ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
enum NotificationKind {
    StudentNumberCorrected,
}

#[derive(Serialize, utoipa::ToSchema, sqlx::FromRow)]
pub(crate) struct StudentNotification {
    id: Uuid,
    kind: NotificationKind,
    created_at: DateTime<Utc>,
}

/// 在更正學號的交易內呼叫，讓通知與更正一起提交或回滾。
pub async fn record_student_number_correction(
    conn: &mut PgConnection,
    user_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO student_notifications (user_id, kind) VALUES ($1, $2)")
        .bind(user_id)
        .bind(NotificationKind::StudentNumberCorrected)
        .execute(conn)
        .await?;
    Ok(())
}

#[utoipa::path(
    get,
    path = "/api/notifications",
    tag = "notifications",
    summary = "列出登入學生自己的未讀通知",
    responses((status = 200, body = [StudentNotification]))
)]
pub(crate) async fn list_unread(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let notifications: Vec<StudentNotification> = sqlx::query_as(
        "SELECT id, kind, created_at FROM student_notifications
         WHERE user_id = $1 AND read_at IS NULL ORDER BY created_at, id",
    )
    .bind(cu.user.id)
    .fetch_all(&state.pool)
    .await?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(notifications)))
}

#[utoipa::path(
    post,
    path = "/api/notifications/{id}/read",
    tag = "notifications",
    summary = "標記登入學生自己的通知為已讀",
    params(("id" = Uuid, Path)),
    responses(
        (status = 204, description = "通知已標記為已讀"),
        (status = 400, description = "通知識別碼不是 UUID", body = crate::error::ErrorBody)
    )
)]
pub(crate) async fn mark_read(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let changed = sqlx::query(
        "UPDATE student_notifications SET read_at = COALESCE(read_at, now())
         WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(cu.user.id)
    .execute(&state.pool)
    .await?;
    if changed.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}
