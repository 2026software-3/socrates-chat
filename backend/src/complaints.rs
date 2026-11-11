//! F-36 / S-17：客訴。已登入的學生與教師送出，系統管理者在管理介面查看與標記處理狀態；
//! 通知信在背景寄出（`mail`）。
//!
//! 描述是使用者自己輸入、要給管理者閱讀的文字（S-02.2 的例外）：管理介面顯示原文，
//! 但日誌、稽核日誌與送往 AI 的請求都不得包含；信件不含學號、電子郵件與對話原文。

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, patch, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{CurrentUser, RequireAdmin},
    error::ApiError,
    mail::{self, ComplaintMail},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/complaints", post(submit))
        .route("/api/admin/complaints", get(list))
        .route("/api/admin/complaints/{id}", patch(update))
        .route("/api/admin/complaints/{id}/resend", post(resend))
}

const CATEGORIES: [&str; 4] = ["connection", "voice", "content", "other"];
const STATUSES: [&str; 2] = ["open", "resolved"];
const MAX_DESCRIPTION_CHARS: usize = 2000;
/// 每人每小時、每日最多送出幾則（S-17.1）。
const MAX_PER_HOUR: i64 = 3;
const MAX_PER_DAY: i64 = 10;
/// 管理者清單一次最多回幾則（新的在前）；客訴量很小，不做分頁。
const LIST_LIMIT: i64 = 200;

const RATE_LIMITED: ApiError = ApiError::new(
    StatusCode::TOO_MANY_REQUESTS,
    "complaint_rate_limited",
    "送出太頻繁，請稍後再試",
);

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewComplaint {
    /// `connection` 連不上、`voice` 語音、`content` 內容、`other` 其他
    category: String,
    /// 必填，去掉前後空白，最多 2000 字
    description: String,
    /// 只在從對話頁開啟表單時帶入；必須是自己的對話。不送對話原文，只送編號
    conversation_id: Option<Uuid>,
}

#[utoipa::path(
    post,
    path = "/api/complaints",
    tag = "complaints",
    summary = "送出客訴（學生與教師）",
    description = "內容會以 email 寄給管理者，並保存在資料庫。每人每小時 3 則、每日 10 則，超過回 429 `complaint_rate_limited`。提交者看不到、也無法追蹤已送出的客訴；寄信失敗不會回錯。",
    request_body = NewComplaint,
    responses(
        (status = 204, description = "已收到"),
        (status = 400, description = "invalid_category、invalid_description", body = crate::error::ErrorBody),
        (status = 404, description = "not_found：附的對話不存在或不是自己的", body = crate::error::ErrorBody),
        (status = 429, description = "complaint_rate_limited", body = crate::error::ErrorBody)
    )
)]
pub(crate) async fn submit(
    cu: CurrentUser,
    State(state): State<AppState>,
    Json(body): Json<NewComplaint>,
) -> Result<StatusCode, ApiError> {
    // 只有學生與教師：尚未開通的帳號與純管理者都不能送
    if !cu.roles.student && !cu.roles.teacher {
        return Err(if cu.roles.admin {
            ApiError::forbidden("forbidden", "沒有權限")
        } else {
            ApiError::forbidden("not_enrolled", "尚未開通")
        });
    }
    if !CATEGORIES.contains(&body.category.as_str()) {
        return Err(ApiError::bad_request("invalid_category", "類別不正確"));
    }
    let description = body.description.trim();
    if description.is_empty() || description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(ApiError::bad_request(
            "invalid_description",
            "描述不可為空，且最多 2000 字",
        ));
    }
    let role = if cu.roles.teacher {
        "teacher"
    } else {
        "student"
    };

    let mut tx = state.pool.begin().await?;
    if let Some(conversation) = body.conversation_id {
        let owned: Option<i32> =
            sqlx::query_scalar("SELECT 1 FROM conversations WHERE id = $1 AND user_id = $2")
                .bind(conversation)
                .bind(cu.user.id)
                .fetch_optional(&mut *tx)
                .await?;
        if owned.is_none() {
            return Err(ApiError::not_found());
        }
    }
    // 同一個人的兩個同時送出不能一起越過上限：以交易層級的建議鎖序列化
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 0))")
        .bind(cu.user.id)
        .execute(&mut *tx)
        .await?;
    let (last_hour, last_day): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE created_at > now() - interval '1 hour'), count(*)
         FROM complaints WHERE user_id = $1 AND created_at > now() - interval '24 hours'",
    )
    .bind(cu.user.id)
    .fetch_one(&mut *tx)
    .await?;
    if last_hour >= MAX_PER_HOUR || last_day >= MAX_PER_DAY {
        return Err(RATE_LIMITED);
    }
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO complaints (user_id, category, description, conversation_id, submitter_role)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(cu.user.id)
    .bind(&body.category)
    .bind(description)
    .bind(body.conversation_id)
    .bind(role)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    // 背景寄信：不讓提交者等 SMTP，失敗也不回錯給提交者
    let state = state.clone();
    tokio::spawn(async move { deliver(&state, id).await });
    Ok(StatusCode::NO_CONTENT)
}

/// 管理介面的一則客訴。描述是原文（S-17.2）；不含提交者的學號與電子郵件。
#[derive(Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct ComplaintView {
    id: Uuid,
    category: String,
    description: String,
    conversation_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    /// `open` 或 `resolved`
    status: String,
    /// `pending`、`sent`、`failed` 或 `skipped`
    mail_status: String,
    submitter_name: Option<String>,
    /// `student` 或 `teacher`
    submitter_role: String,
    /// 只在附了對話編號時才有
    classroom_name: Option<String>,
}

/// 編譯期拼成靜態 SQL（sqlx 不收執行期組出來的字串）。
macro_rules! view_select {
    ($tail:literal) => {
        concat!(
            "SELECT c.id, c.category, c.description, c.conversation_id, c.created_at, c.status, c.mail_status,
                    u.display_name AS submitter_name, c.submitter_role, cl.name AS classroom_name
             FROM complaints c
             JOIN users u ON u.id = c.user_id
             LEFT JOIN conversations cv ON cv.id = c.conversation_id
             LEFT JOIN classrooms cl ON cl.id = cv.classroom_id ",
            $tail
        )
    };
}

#[derive(Deserialize, utoipa::IntoParams)]
pub(crate) struct ListQuery {
    /// 只看 `open` 或 `resolved`；不給就是全部
    status: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/admin/complaints",
    tag = "complaints",
    summary = "客訴清單（管理者）",
    description = "新的在前，最多 200 則。描述顯示原文（S-02.2 的例外）。",
    params(ListQuery),
    responses(
        (status = 200, body = [ComplaintView]),
        (status = 400, description = "invalid_status", body = crate::error::ErrorBody)
    )
)]
pub(crate) async fn list(
    _: RequireAdmin,
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<ComplaintView>>, ApiError> {
    if q.status.as_deref().is_some_and(|s| !STATUSES.contains(&s)) {
        return Err(ApiError::bad_request("invalid_status", "處理狀態不正確"));
    }
    let rows = sqlx::query_as(view_select!(
        "WHERE ($1::text IS NULL OR c.status = $1) ORDER BY c.created_at DESC, c.id LIMIT $2"
    ))
    .bind(q.status)
    .bind(LIST_LIMIT)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct StatusBody {
    /// `open` 或 `resolved`
    status: String,
}

#[utoipa::path(
    patch,
    path = "/api/admin/complaints/{id}",
    tag = "complaints",
    summary = "標記客訴的處理狀態（管理者）",
    description = "狀態有變更時寫稽核紀錄（操作者、時間、客訴編號；不含描述）。",
    params(("id" = Uuid, Path)),
    request_body = StatusBody,
    responses(
        (status = 200, body = ComplaintView),
        (status = 400, description = "invalid_status", body = crate::error::ErrorBody)
    )
)]
pub(crate) async fn update(
    RequireAdmin(cu): RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<StatusBody>,
) -> Result<Json<ComplaintView>, ApiError> {
    if !STATUSES.contains(&body.status.as_str()) {
        return Err(ApiError::bad_request("invalid_status", "處理狀態不正確"));
    }
    let mut tx = state.pool.begin().await?;
    let current: String =
        sqlx::query_scalar("SELECT status FROM complaints WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(ApiError::not_found)?;
    if current != body.status {
        sqlx::query("UPDATE complaints SET status = $2 WHERE id = $1")
            .bind(id)
            .bind(&body.status)
            .execute(&mut *tx)
            .await?;
        let action = if body.status == "resolved" {
            "complaint_resolved"
        } else {
            "complaint_reopened"
        };
        // 稽核只記操作者與客訴編號，不記描述
        sqlx::query("INSERT INTO audit_logs (actor_id, action, target_email) VALUES ($1, $2, $3)")
            .bind(cu.user.id)
            .bind(action)
            .bind(id.to_string())
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    let view = sqlx::query_as(view_select!("WHERE c.id = $1"))
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(view))
}

#[utoipa::path(
    post,
    path = "/api/admin/complaints/{id}/resend",
    tag = "complaints",
    summary = "重寄客訴通知信（管理者）",
    description = "寄信狀態先回到 `pending`，在背景重寄；結果看清單裡的 `mail_status`。沒有設定 SMTP 時結果是 `skipped`。",
    params(("id" = Uuid, Path)),
    responses((status = 204, description = "已排入重寄"))
)]
pub(crate) async fn resend(
    _: RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let updated = sqlx::query("UPDATE complaints SET mail_status = 'pending' WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if updated == 0 {
        return Err(ApiError::not_found());
    }
    let state = state.clone();
    tokio::spawn(async move { deliver(&state, id).await });
    Ok(StatusCode::NO_CONTENT)
}

#[derive(FromRow)]
struct MailRow {
    id: Uuid,
    category: String,
    description: String,
    created_at: DateTime<Utc>,
    conversation_id: Option<Uuid>,
    submitter_name: Option<String>,
    submitter_role: String,
    classroom_name: Option<String>,
}

async fn set_mail_status(state: &AppState, id: Uuid, status: &str) {
    if let Err(e) = sqlx::query("UPDATE complaints SET mail_status = $2 WHERE id = $1")
        .bind(id)
        .bind(status)
        .execute(&state.pool)
        .await
    {
        tracing::warn!(complaint_id = %id, class = %crate::error::db_error_class(&e), "failed to save complaint mail status");
    }
}

/// 寄出一則客訴的通知信並記下結果（`sent`／`failed`／`skipped`）。
/// 日誌只有客訴編號與結果的類別：不記描述、收件信箱或 SMTP 憑證。
pub async fn deliver(state: &AppState, id: Uuid) {
    let Some(config) = state.config.smtp.as_ref() else {
        set_mail_status(state, id, "skipped").await;
        tracing::info!(complaint_id = %id, result = "skipped", "complaint mail not sent");
        return;
    };
    let row: Option<MailRow> = match sqlx::query_as(
        "SELECT c.id, c.category, c.description, c.created_at, c.conversation_id,
                u.display_name AS submitter_name, c.submitter_role, cl.name AS classroom_name
         FROM complaints c
         JOIN users u ON u.id = c.user_id
         LEFT JOIN conversations cv ON cv.id = c.conversation_id
         LEFT JOIN classrooms cl ON cl.id = cv.classroom_id
         WHERE c.id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(row) => row,
        Err(e) => {
            tracing::warn!(complaint_id = %id, class = %crate::error::db_error_class(&e), "failed to load complaint for mail");
            set_mail_status(state, id, "failed").await;
            return;
        }
    };
    // 客訴在寄出前已被刪除（帳號刪除）：沒有東西可寄
    let Some(row) = row else { return };
    let mail = ComplaintMail {
        id: row.id,
        category: row.category,
        description: row.description,
        created_at: row.created_at,
        conversation_id: row.conversation_id,
        submitter_name: row.submitter_name,
        submitter_role: row.submitter_role,
        classroom_name: row.classroom_name,
    };
    match mail::send(config, &mail).await {
        Ok(()) => {
            set_mail_status(state, id, "sent").await;
            tracing::info!(complaint_id = %id, result = "sent", "complaint mail sent");
        }
        Err(failure) => {
            set_mail_status(state, id, "failed").await;
            tracing::warn!(complaint_id = %id, result = "failed", class = failure.class(), "complaint mail failed");
        }
    }
}
