//! 學生帳號管理（F-01.4、F-22.1、F-22.5；規格見 S-01.3、S-01.4、S-02.5）。
//!
//! - 教師或管理者：查看修課學生的帳號狀態、刪除學生帳號（需再次確認）。
//! - 管理者：停用與復原帳號；停用時立即清除該帳號所有 session，資料全部保留。
//!
//! 每次停用、復原與刪除都寫入稽核日誌（操作者、時間、選填原因）。

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, put},
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{RequireAdmin, RequireTeacher},
    error::ApiError,
    roster::normalize_email,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/students", get(list_students))
        .route("/api/students/{email}", delete(delete_student))
        .route("/api/admin/users/{id}/status", put(set_status))
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct StudentRow {
    /// 使用者 UUID；尚未建立帳號時為 null。
    id: Option<Uuid>,
    email: String,
    display_name: Option<String>,
    /// 學生是否已有帳號（登入過，或管理者匯入時建立了內建帳號）
    has_account: bool,
    disabled: bool,
    /// 已完成（總結已產生）的對話數；教師只看得到數量，看不到內容
    completed_conversations: i64,
}

/// 修課名單上的學生與帳號狀態。
#[utoipa::path(
    get,
    path = "/api/students",
    tag = "roster",
    summary = "名單上的學生與帳號狀態",
    responses((status = 200, description = "學生列表", body = Vec<StudentRow>), crate::error::CommonErrors)
)]
pub(crate) async fn list_students(
    _: RequireTeacher,
    State(state): State<AppState>,
) -> Result<Json<Vec<StudentRow>>, ApiError> {
    let rows = sqlx::query_as(
        "SELECT u.id, e.email, u.display_name,
                (u.id IS NOT NULL) AS has_account,
                COALESCE(u.disabled_at IS NOT NULL, false) AS disabled,
                COALESCE((SELECT count(*) FROM summaries s
                          JOIN conversations c ON c.id = s.conversation_id
                          WHERE c.user_id = u.id AND s.status = 'ready'), 0)
                    AS completed_conversations
         FROM enrollments e LEFT JOIN users u ON u.email = e.email
         ORDER BY e.email",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

fn email_or_400(raw: &str) -> Result<String, ApiError> {
    normalize_email(raw).ok_or(ApiError::bad_request("invalid_email", "電子郵件格式不正確"))
}

pub(crate) async fn audit(
    conn: impl sqlx::PgExecutor<'_>,
    actor: Uuid,
    action: &str,
    target_email: &str,
    reason: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_logs (actor_id, action, target_email, reason) VALUES ($1, $2, $3, $4)",
    )
    .bind(actor)
    .bind(action)
    .bind(target_email)
    .bind(reason)
    .execute(conn)
    .await?;
    Ok(())
}

/// 原因為選填；去空白後為空視為沒有，並限制長度。
fn clean_reason(reason: Option<String>) -> Option<String> {
    reason
        .map(|r| r.trim().chars().take(500).collect::<String>())
        .filter(|r| !r.is_empty())
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct StatusBody {
    disabled: bool,
    reason: Option<String>,
}

#[utoipa::path(
    put,
    path = "/api/admin/users/{id}/status",
    tag = "admin",
    summary = "設定帳號停用狀態（冪等）",
    params(("id" = Uuid, Path, description = "使用者 ID")),
    request_body = StatusBody,
    responses((status = 204, description = "狀態已設定；相同狀態不重複稽核"), (status = 409, description = "cannot_disable_self / last_admin", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn set_status(
    RequireAdmin(cu): RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<StatusBody>,
) -> Result<StatusCode, ApiError> {
    if body.disabled && id == cu.user.id {
        return Err(ApiError::conflict(
            "cannot_disable_self",
            "不能停用自己的帳號",
        ));
    }
    let reason = clean_reason(body.reason);
    let mut tx = state.pool.begin().await?;
    // 以相同順序鎖住管理者，避免並行操作把最後兩位管理者同時停用。
    sqlx::query("SELECT id FROM users WHERE is_admin ORDER BY id FOR UPDATE")
        .fetch_all(&mut *tx)
        .await?;
    let target: Option<(String, bool, bool)> = sqlx::query_as(
        "SELECT email, is_admin, disabled_at IS NOT NULL FROM users WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((email, is_admin, disabled)) = target else {
        return Err(ApiError::not_found());
    };
    if disabled == body.disabled {
        return Ok(StatusCode::NO_CONTENT);
    }
    if body.disabled && is_admin {
        let (others,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM users WHERE is_admin AND disabled_at IS NULL AND id <> $1",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if others == 0 {
            return Err(ApiError::conflict(
                "last_admin",
                "至少需保留一位啟用中的管理者",
            ));
        }
    }
    sqlx::query("UPDATE users SET disabled_at = CASE WHEN $2 THEN now() ELSE NULL END, updated_at = now() WHERE id = $1")
        .bind(id).bind(body.disabled).execute(&mut *tx).await?;
    if body.disabled {
        sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    audit(
        &mut *tx,
        cu.user.id,
        if body.disabled {
            "account_disabled"
        } else {
            "account_enabled"
        },
        &email,
        reason.as_deref(),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct DeleteBody {
    /// 再次確認：必須與路徑上的電子郵件相同（S-02.5）
    confirm_email: String,
}

/// 刪除學生帳號：一併刪除對話、總結與修課名單項目（S-02.5）。教師與管理者帳號不能由這裡刪除。
#[utoipa::path(
    delete,
    path = "/api/students/{email}",
    tag = "roster",
    summary = "刪除學生帳號與其資料",
    params(("email" = String, Path, description = "電子郵件")),
    request_body = DeleteBody,
    responses((status = 204, description = "已刪除"), (status = 400, description = "confirmation_mismatch", body = crate::error::ErrorBody), (status = 409, description = "not_a_student", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn delete_student(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(email): Path<String>,
    Json(body): Json<DeleteBody>,
) -> Result<StatusCode, ApiError> {
    let email = email_or_400(&email)?;
    if normalize_email(&body.confirm_email).as_deref() != Some(email.as_str()) {
        return Err(ApiError::bad_request(
            "confirmation_mismatch",
            "確認的電子郵件與要刪除的帳號不符",
        ));
    }
    let mut tx = state.pool.begin().await?;
    let (staff,): (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM teachers WHERE email = $1)
             OR EXISTS (SELECT 1 FROM users WHERE email = $1 AND is_admin)",
    )
    .bind(&email)
    .fetch_one(&mut *tx)
    .await?;
    if staff || state.config.admin_emails.contains(&email) {
        return Err(ApiError::conflict("not_a_student", "只能刪除學生帳號"));
    }
    let on_roster = sqlx::query("DELETE FROM enrollments WHERE email = $1")
        .bind(&email)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    // 對話、訊息、總結與 session 由外鍵連動刪除
    let had_account = sqlx::query("DELETE FROM users WHERE email = $1")
        .bind(&email)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if on_roster + had_account == 0 {
        return Err(ApiError::not_found());
    }
    sqlx::query("DELETE FROM login_failures WHERE email = $1")
        .bind(&email)
        .execute(&mut *tx)
        .await?;
    audit(&mut *tx, cu.user.id, "student_deleted", &email, None).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
