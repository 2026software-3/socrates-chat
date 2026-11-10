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
    roster::{is_staff_email, normalize_email},
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
    responses((status = 200, description = "學生分頁，依帳號 UUID 排序，帶符合條件的 total", body = StudentPage), (status = 400, description = "invalid_limit / invalid_cursor / invalid_filter", body = crate::error::ErrorBody), crate::error::CommonErrors)
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
    let cursor: Option<(Uuid, String)> = query.page.cursor("students")?;
    // 游標只對產生它時的搜尋條件有效；換條件要從第一頁開始
    if cursor.as_ref().is_some_and(|c| c.1 != fingerprint) {
        return Err(ApiError::bad_request("invalid_cursor", "分頁游標不正確"));
    }
    let classroom = visible_classroom(&state, &cu, &raw_id).await?;
    let search = q.map(pagination::like_pattern);
    let (rows, total) = tokio::try_join!(
        roster_store::list_students(
            &state.pool,
            classroom.id,
            cursor.as_ref().map(|c| c.0),
            (limit + 1) as i64,
            search.as_deref(),
        ),
        roster_store::count_students_matching(&state.pool, classroom.id, search.as_deref()),
    )?;
    let Page { items, next_cursor } =
        pagination::page(rows, limit, "students", |row| (row.id, fingerprint.clone()))?;
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
    target: &str,
    reason: Option<&str>,
) -> Result<(), sqlx::Error> {
    audit_counted(conn, actor, action, target, reason, None).await
}

/// 稽核紀錄另帶數量；刪除教室或活動時記錄一併刪除的對話數（S-14）。
pub(crate) async fn audit_counted(
    conn: impl sqlx::PgExecutor<'_>,
    actor: Uuid,
    action: &str,
    target: &str,
    reason: Option<&str>,
    item_count: Option<i64>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_logs (actor_id, action, target_email, reason, item_count) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(actor)
    .bind(action)
    .bind(target)
    .bind(reason)
    .bind(item_count.map(|n| n as i32))
    .execute(conn)
    .await?;
    Ok(())
}

/// 刪除請求的 JSON body 缺少、格式錯誤或不是 JSON 時，與名稱不符同樣回 400，維持專案的錯誤格式。
pub(crate) fn confirmation<T>(
    body: Result<Json<T>, axum::extract::rejection::JsonRejection>,
) -> Result<T, ApiError> {
    body.map(|Json(value)| value)
        .map_err(|_| ApiError::bad_request("confirmation_mismatch", "請提供要刪除對象的名稱以確認"))
}

/// 輸入的名稱必須與對象名稱相同（比對時去除前後空白）。
pub(crate) fn ensure_confirmed(given: &str, expected: &str) -> Result<(), ApiError> {
    if given.trim() == expected.trim() {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "confirmation_mismatch",
            "確認的名稱與要刪除的對象不符",
        ))
    }
}

/// 教室與活動刪除共用的 SQL 與稽核動作。
pub(crate) struct DeleteSpec {
    /// 要刪的列是否存在（D1 版當作稽核寫入的條件；PostgreSQL 版用來鎖列）
    pub lock: &'static str,
    pub count_conversations: &'static str,
    pub delete: &'static str,
    pub action: &'static str,
}

/// 計算一併刪除的對話數、寫入稽核日誌並刪除，三者在同一個 D1 batch 內（S-14）。
pub(crate) async fn delete_with_audit(
    pool: &sqlx::PgPool,
    spec: &DeleteSpec,
    id: Uuid,
    actor: Uuid,
    target: &str,
    reason: Option<String>,
) -> Result<(), ApiError> {
    // 稽核（含一併刪除的對話數）與刪除在同一個 D1 batch（原子），依序執行：先在刪除前計數並寫稽核，
    // 再刪除。PostgreSQL 版以 FOR UPDATE 鎖列再計數；batch 內不會有其他寫入插進來，計數一樣準確。
    let audit_sql = format!(
        "INSERT INTO audit_logs (actor_id, action, target_email, reason, item_count)
         SELECT $2, $3, $4, $5, ({}) WHERE EXISTS ({})",
        spec.count_conversations, spec.lock
    );
    let results = pool
        .batch(vec![
            sqlx::query(&audit_sql)
                .bind(id)
                .bind(actor)
                .bind(spec.action)
                .bind(target)
                .bind(clean_reason(reason)),
            sqlx::query(spec.delete).bind(id),
        ])
        .await?;
    if results.get(1).is_none_or(|r| r.rows_affected() == 0) {
        return Err(ApiError::not_found());
    }
    Ok(())
}

/// 原因為選填；去空白後為空視為沒有，並限制長度。
pub(crate) fn clean_reason(reason: Option<String>) -> Option<String> {
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
    // PostgreSQL 版在這裡以 FOR UPDATE 鎖住所有管理者；D1 沒有列鎖，「至少保留一位管理者」改由下方
    // UPDATE 的條件保證（單一語句是原子的），兩位管理者同時互相停用時只有一個成功。
    // 只有學號、沒有信箱的帳號 email 是 NULL：稽核對象改用帳號 ID（與重設密碼相同）
    let target: Option<(Option<String>, bool, bool)> =
        sqlx::query_as("SELECT email, is_admin, disabled_at IS NOT NULL FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some((email, is_admin, disabled)) = target else {
        return Err(ApiError::not_found());
    };
    let email = email.unwrap_or_else(|| id.to_string());
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
    let changed = sqlx::query(
        "UPDATE users SET disabled_at = CASE WHEN $2 THEN now() ELSE NULL END, updated_at = now()
         WHERE id = $1
           AND (NOT $2 OR NOT is_admin
                OR EXISTS (SELECT 1 FROM users o
                           WHERE o.is_admin AND o.disabled_at IS NULL AND o.id <> $1))",
    )
    .bind(id)
    .bind(body.disabled)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if changed == 0 {
        return Err(ApiError::conflict(
            "last_admin",
            "至少需保留一位啟用中的管理者",
        ));
    }
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
    /// 再次確認：必須與學生學號相同（不分大小寫）。
    #[schema(required)]
    confirm_student_number: Option<String>,
}

/// 刪除學生帳號：一併刪除對話、總結與所有教室的修課名單項目（S-02.5）。教師與管理者帳號不能由這裡刪除；
/// 教師只能刪除完全屬於自己教室的學生，管理者不受限。
#[utoipa::path(
    delete,
    path = "/api/students/{email}",
    tag = "roster",
    summary = "刪除學生帳號與其資料",
    params(("email" = String, Path, description = "學生帳號 UUID 或電子郵件")),
    request_body = DeleteBody,
    responses((status = 204, description = "已刪除"), (status = 400, description = "confirmation_mismatch", body = crate::error::ErrorBody), (status = 404, description = "not_found：帳號不存在，或教師要刪除的學生不完全屬於自己建立的教室", body = crate::error::ErrorBody), (status = 409, description = "not_a_student", body = crate::error::ErrorBody))
)]
pub(crate) async fn delete_student(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(email): Path<String>,
    Json(body): Json<DeleteBody>,
) -> Result<StatusCode, ApiError> {
    let id = Uuid::parse_str(&email).ok();
    let normalized_email = if id.is_none() {
        Some(email_or_400(&email)?)
    } else {
        None
    };
    let mut tx = state.pool.begin().await?;
    let account: Option<(Uuid, Option<String>, Option<String>, bool)> = sqlx::query_as(
        "SELECT id, email, student_id, is_admin FROM users WHERE id = $1 OR email = $2",
    )
    .bind(id)
    .bind(normalized_email.as_deref())
    .fetch_optional(&mut *tx)
    .await?;
    let (user_id, email, student_number, is_admin) = account.ok_or_else(ApiError::not_found)?;
    if is_admin
        || if let Some(email) = &email {
            is_staff_email(&state, &mut tx, email).await?
        } else {
            false
        }
    {
        return Err(ApiError::conflict("not_a_student", "只能刪除學生帳號"));
    }
    // 刪除會連同所有教室的資料一併刪除：教師只能刪除完全屬於自己教室的學生（範圍待確認，S-02.5）；
    // 其他情況與不存在的帳號同樣回 404，不外露資源是否存在。
    if !cu.roles.admin
        && !roster_store::user_belongs_only_to_owner(&mut tx, user_id, cu.user.id).await?
    {
        return Err(ApiError::not_found());
    }
    if !student_number
        .as_deref()
        .zip(body.confirm_student_number.as_deref())
        .is_some_and(|(expected, supplied)| expected.eq_ignore_ascii_case(supplied.trim()))
    {
        return Err(ApiError::bad_request(
            "confirmation_mismatch",
            "確認的學號與要刪除的帳號不符",
        ));
    }
    // 限流紀錄用學號與電子郵件的雜湊為鍵，要在帳號還在時清掉
    crate::auth_store::clear_login_failures_of_user(&mut tx, user_id).await?;
    // 對話、訊息、總結與 session 由外鍵連動刪除。
    // PostgreSQL 版以 FOR UPDATE 鎖住帳號；D1 沒有列鎖，上面檢查之後學生可能剛被加進別位教師的教室，
    // 所以「完全屬於自己教室」與「不是管理者」的條件再放進 DELETE（單一語句是原子的）。
    let deleted = sqlx::query(
        "DELETE FROM users WHERE id = $1 AND NOT is_admin
           AND ($2 OR (EXISTS (SELECT 1 FROM enrollments e WHERE e.user_id = $1)
                       AND NOT EXISTS (SELECT 1 FROM enrollments e
                                       JOIN classrooms c ON c.id = e.classroom_id
                                       WHERE e.user_id = $1 AND c.created_by <> $3)))",
    )
    .bind(user_id)
    .bind(cu.roles.admin)
    .bind(cu.user.id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if deleted == 0 {
        return Err(ApiError::not_found());
    }
    audit(
        &mut *tx,
        cu.user.id,
        "student_deleted",
        email.as_deref().unwrap_or(&user_id.to_string()),
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
