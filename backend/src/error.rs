//! 統一的 API 錯誤格式（S-08.1）：JSON `{ "error": { code, message, request_id } }`。
//! 不得包含堆疊、金鑰或內部資訊。

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

tokio::task_local! {
    /// 目前請求的 ID，由 `request_id` middleware 設定。
    pub static REQUEST_ID: String;
}

/// 供 request logging 使用的安全錯誤分類；只含固定的 API error code。
#[derive(Clone, Copy)]
pub(crate) struct ErrorCode(pub &'static str);

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: &'static str,
}

impl ApiError {
    pub const fn new(status: StatusCode, code: &'static str, message: &'static str) -> Self {
        Self {
            status,
            code,
            message,
        }
    }

    pub const fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", "請先登入")
    }

    pub const fn forbidden(code: &'static str, message: &'static str) -> Self {
        Self::new(StatusCode::FORBIDDEN, code, message)
    }

    pub const fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "找不到資源")
    }

    pub const fn bad_request(code: &'static str, message: &'static str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    pub const fn conflict(code: &'static str, message: &'static str) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }

    // 以下是多個地方共用的錯誤：code 與訊息只在這裡定義一次。

    pub const fn no_permission() -> Self {
        Self::forbidden("forbidden", "沒有權限")
    }

    pub const fn reply_in_progress() -> Self {
        Self::conflict("reply_in_progress", "AI 正在回覆中")
    }

    pub const fn conversation_ended() -> Self {
        Self::conflict("conversation_ended", "對話已結束")
    }

    pub const fn classroom_archived() -> Self {
        Self::conflict("classroom_archived", "教室已封存")
    }

    pub const fn invalid_title() -> Self {
        Self::bad_request("invalid_title", "標題不可為空")
    }

    pub const fn invalid_cursor() -> Self {
        Self::bad_request("invalid_cursor", "分頁游標不正確")
    }

    pub const fn invalid_message() -> Self {
        Self::bad_request("invalid_message", "訊息長度不符")
    }

    pub const fn invalid_language() -> Self {
        Self::bad_request("invalid_language", "不支援的語言")
    }

    pub const fn invalid_filter() -> Self {
        Self::bad_request("invalid_filter", "篩選條件不正確")
    }

    pub const fn invalid_email() -> Self {
        Self::bad_request("invalid_email", "電子郵件格式不正確")
    }

    pub const fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "系統發生錯誤，請稍後再試",
        )
    }
}

/// 資料庫錯誤的安全摘要：只含 SQLSTATE，不含錯誤訊息。
/// PostgreSQL 的約束錯誤訊息可能帶出整列資料（含對話或總結內容），因此不能直接寫進日誌。
pub fn db_error_class(e: &sqlx::Error) -> String {
    e.as_database_error()
        .and_then(|d| d.code().map(|c| c.to_string()))
        .unwrap_or_else(|| "non-database".to_string())
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        // 只記錄錯誤種類，不把內部細節回給前端或日誌
        tracing::error!(class = %db_error_class(&e), "database error");
        Self::internal()
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let request_id = REQUEST_ID.try_with(|id| id.clone()).unwrap_or_default();
        let mut response = (
            self.status,
            Json(json!({
                "error": {
                    // S-12.1：只回 code 與 request_id，由前端依 code 查翻譯
                    "code": self.code,
                    "request_id": request_id,
                }
            })),
        )
            .into_response();
        response.extensions_mut().insert(ErrorCode(self.code));
        response
    }
}

/// OpenAPI 用的錯誤本文型別（只描述格式，實際輸出見 `ApiError::into_response`）。
#[derive(utoipa::ToSchema)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(utoipa::ToSchema)]
pub struct ErrorDetail {
    /// 錯誤碼，前端依此顯示翻譯文字（S-12.1）
    pub code: String,
    pub request_id: String,
}

/// 幾乎所有需要登入的端點共有的錯誤回應。
#[derive(utoipa::IntoResponses)]
#[allow(dead_code)]
pub enum CommonErrors {
    /// 未登入（`unauthorized`）
    #[response(status = 401)]
    Unauthorized(#[to_schema] ErrorBody),
    /// 角色不符、名單外、帳號停用、須先改密碼或 CSRF（`forbidden`／`not_enrolled`／`account_disabled`／`password_change_required`／`csrf`）
    #[response(status = 403)]
    Forbidden(#[to_schema] ErrorBody),
    /// 資源不存在或不屬於你（`not_found`）
    #[response(status = 404)]
    NotFound(#[to_schema] ErrorBody),
    /// 系統錯誤（`internal`）
    #[response(status = 500)]
    Internal(#[to_schema] ErrorBody),
}
