//! F-40.9 (#605): 系統管理者查看教師 key 的設定狀態並刪除（S-03.1「教師 API key 與使用範圍」）。
//!
//! 管理者看得到各教師有哪些 key 的供應商、遮蔽識別與狀態，**看不到用量與完整 key，也不能新增或更換**。
//! 刪除會一併刪掉該 key 的用量紀錄（教室統計隨之扣除）；同一個交易內通知該教師（只存類型與時間，
//! 文字由前端依語言顯示）並寫稽核紀錄：操作者、時間、受影響的教師帳號，不含 key 與用量。

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{delete, get},
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::{AppState, accounts, auth::RequireAdmin, error::ApiError, student_notifications};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/teacher-keys", get(list_keys))
        .route("/api/admin/teacher-keys/{id}", delete(delete_key))
}

#[derive(Serialize, utoipa::ToSchema, sqlx::FromRow)]
pub(crate) struct AdminKeyItem {
    id: Uuid,
    /// 持有這把 key 的教師帳號
    owner_email: String,
    provider: String,
    /// 遮蔽後的識別（只有末四碼）
    mask: String,
    /// `ok`／`invalid`／`quota_exhausted`／`rate_limited`
    status: String,
    status_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

#[utoipa::path(
    get,
    path = "/api/admin/teacher-keys",
    tag = "admin",
    summary = "管理者查看各教師 key 的設定狀態（不含用量與完整 key）",
    responses(
        (status = 200, body = [AdminKeyItem]),
        (status = 401, description = "未登入", body = crate::error::ErrorBody),
        (status = 403, description = "不是管理者", body = crate::error::ErrorBody)
    )
)]
pub(crate) async fn list_keys(
    RequireAdmin(_): RequireAdmin,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let items: Vec<AdminKeyItem> = sqlx::query_as(
        "SELECT k.id, u.email AS owner_email, k.provider, k.mask, k.status, k.status_at, k.created_at
           FROM teacher_api_keys k JOIN users u ON u.id = k.owner_id
          ORDER BY u.email, k.created_at, k.id",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(items)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/teacher-keys/{id}",
    tag = "admin",
    summary = "管理者刪除教師 key；key 與用量紀錄一併刪除，並通知教師、寫入稽核紀錄",
    params(("id" = Uuid, Path)),
    responses(
        (status = 204, description = "已刪除"),
        (status = 401, description = "未登入", body = crate::error::ErrorBody),
        (status = 403, description = "不是管理者", body = crate::error::ErrorBody),
        (status = 404, description = "key 不存在", body = crate::error::ErrorBody)
    )
)]
pub(crate) async fn delete_key(
    RequireAdmin(cu): RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let mut tx = state.pool.begin().await?;
    let key = crate::teacher_key_store::delete_any(&mut *tx, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let owner_email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(key.owner_id)
        .fetch_one(&mut *tx)
        .await?;
    student_notifications::record_teacher_key_deleted(&mut tx, key.owner_id).await?;
    // 稽核只記操作者與受影響的教師帳號；不記 key 內容、遮蔽識別或用量
    accounts::audit(
        &mut *tx,
        cu.user.id,
        "teacher_key_deleted",
        &owner_email,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
