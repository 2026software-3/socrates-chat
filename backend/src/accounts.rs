//! 學生帳號管理（F-01.4、F-22.1、F-22.5；規格見 S-01.3、S-01.4、S-02.5）。
//!
//! - 教師或管理者：查看修課學生的帳號狀態、刪除學生帳號（需再次確認）。
//! - 管理者：停用與復原帳號；停用時立即清除該帳號所有 session，資料全部保留。
//!
//! 每次停用、復原與刪除都寫入稽核日誌（操作者、時間、選填原因）。

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, put},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{RequireAdmin, RequireTeacher},
    classrooms::visible_classroom,
    error::ApiError,
    pagination::{self, Page, Pagination},
    roster::normalize_email,
    roster_store::{self, StudentRow},
};

/// 學生分頁；`total` 是符合條件（同一個 `q`）的學生總數，每一頁都帶，不受游標影響。
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct StudentPage {
    items: Vec<StudentRow>,
    next_cursor: Option<String>,
    total: i64,
}

/// 分頁參數加上搜尋。
#[derive(Deserialize)]
pub(crate) struct StudentQuery {
    #[serde(flatten)]
    page: Pagination,
    /// 搜尋電子郵件或顯示名稱（不分大小寫的子字串，最多 100 字元）
    q: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/classrooms/{id}/students", get(list_students))
        .route("/api/students/{email}", delete(delete_student))
        .route("/api/admin/users/{id}/status", put(set_status))
}

/// 教室修課名單上的學生與帳號狀態。
#[utoipa::path(
    get,
    path = "/api/classrooms/{id}/students",
    tag = "roster",
    summary = "教室名單上的學生與帳號狀態",
    params(
        ("id" = Uuid, Path, description = "教室 ID"),
        ("limit" = Option<u16>, Query, description = "1–200，預設 50"),
        ("cursor" = Option<String>, Query, description = "前頁 next_cursor；只對產生它時的 q 有效，換搜尋條件要從第一頁開始"),
        ("q" = Option<String>, Query, description = "搜尋電子郵件或顯示名稱（不分大小寫的子字串，最多 100 字元），涵蓋所有頁"),
    ),
    responses((status = 200, description = "學生分頁，依 email 排序，帶符合條件的 total", body = StudentPage), (status = 400, description = "invalid_limit / invalid_cursor / invalid_filter", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn list_students(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
    Query(query): Query<StudentQuery>,
) -> Result<Json<StudentPage>, ApiError> {
    let limit = query.page.limit()?;
    let q = pagination::search_term(query.q.as_deref())?;
    let fingerprint = pagination::fingerprint(&q.map(str::to_lowercase));
    let cursor: Option<(String, String)> = query.page.cursor("students")?;
    // 游標只對產生它時的搜尋條件有效；換條件要從第一頁開始
    if cursor.as_ref().is_some_and(|c| c.1 != fingerprint) {
        return Err(ApiError::bad_request("invalid_cursor", "分頁游標不正確"));
    }
    let classroom = visible_classroom(&state, &cu, &raw_id).await?;
    let search = q.map(pagination::like_pattern);
    let rows = roster_store::list_students(
        &state.pool,
        classroom.id,
        cursor.as_ref().map(|c| c.0.as_str()),
        (limit + 1) as i64,
        search.as_deref(),
    )
    .await?;
    let total =
        roster_store::count_students_matching(&state.pool, classroom.id, search.as_deref()).await?;
    let Page { items, next_cursor } = pagination::page(rows, limit, "students", |row| {
        (row.email.clone(), fingerprint.clone())
    })?;
    Ok(Json(StudentPage {
        items,
        next_cursor,
        total,
    }))
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
    responses((status = 204, description = "狀態已設定；相同狀態不重複稽核"), (status = 409, description = "cannot_disable_self / last_admin", body = crate::error::ErrorBody))
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

/// 刪除學生帳號：一併刪除對話、總結與所有教室的修課名單項目（S-02.5）。教師與管理者帳號不能由這裡刪除；
/// 教師只能刪除完全屬於自己教室的學生，管理者不受限。
#[utoipa::path(
    delete,
    path = "/api/students/{email}",
    tag = "roster",
    summary = "刪除學生帳號與其資料",
    params(("email" = String, Path, description = "電子郵件")),
    request_body = DeleteBody,
    responses((status = 204, description = "已刪除"), (status = 400, description = "confirmation_mismatch", body = crate::error::ErrorBody), (status = 404, description = "not_found：帳號不存在，或教師要刪除的學生不完全屬於自己建立的教室", body = crate::error::ErrorBody), (status = 409, description = "not_a_student", body = crate::error::ErrorBody))
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
    // 刪除會連同所有教室的資料一併刪除：教師只能刪除完全屬於自己教室的學生（範圍待確認，S-02.5）；
    // 其他情況與不存在的帳號同樣回 404，不外露資源是否存在。
    if !cu.roles.admin && !roster_store::belongs_only_to_owner(&mut tx, &email, cu.user.id).await? {
        return Err(ApiError::not_found());
    }
    // 所有教室的名單項目一併移除（學生可屬於多個教室）
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
