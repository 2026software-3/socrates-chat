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
        (
            self.status,
            Json(json!({
                "error": {
                    // S-12.1：只回 code 與 request_id，由前端依 code 查翻譯
                    "code": self.code,
                    "request_id": request_id,
                }
            })),
        )
            .into_response()
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

/// 過濾敏感日誌的函數 (目前是空的，為了讓測試先失敗)
pub fn filter_sensitive_log(raw_log: &str) -> String {
    // 嘗試將文字解析為 JSON 格式
    if let Ok(mut json_val) = serde_json::from_str::<serde_json::Value>(raw_log) {
        // 如果成功解析為 JSON 物件，就進行欄位替換
        if let Some(obj) = json_val.as_object_mut() {
            
            // 1. 電子郵件：完全不記錄 (替換成 *** 遮蔽)
            if obj.contains_key("user_email") {
                obj.insert("user_email".to_string(), serde_json::json!("***"));
            }
            
            // 2. 密碼與金鑰：不記錄 (替換成 ***)
            if obj.contains_key("password") {
                obj.insert("password".to_string(), serde_json::json!("***"));
            }
            
            // 3. 對話訊息：依 S-02.2 規定替換成 "message"
            if obj.contains_key("chat_message") {
                obj.insert("chat_message".to_string(), serde_json::json!("message"));
            }
        }
        // 將過濾後的 JSON 物件重新轉回字串並回傳
        return json_val.to_string();
    }
    
    // 如果傳入的不是 JSON 格式，就原封不動回傳
    raw_log.to_string()
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_sensitive_data_filtering() {
        // 準備帶有敏感資訊的合成資料
        let raw_log = r#"{
            "user_email": "student@ntnu.edu.tw",
            "password": "secret_password_123",
            "chat_message": "老師，我覺得存在主義很難懂",
            "status": 200
        }"#;

        // 呼叫過濾函數
        let filtered_log = filter_sensitive_log(raw_log);

        // 斷言：日誌連電子郵件也不記錄
        assert!(!filtered_log.contains("student@ntnu.edu.tw"), "Email should be filtered!");
        
        // 斷言：不記錄密碼
        assert!(!filtered_log.contains("secret_password_123"), "Password should be filtered!");
        
        // 斷言：對話訊息原文必須被過濾掉 (依據 S-02.2 規定替換)
        assert!(!filtered_log.contains("老師，我覺得存在主義很難懂"), "Chat message should be filtered!");
        
        // 斷言：原本正常的狀態碼應該要保留
        assert!(filtered_log.contains("200"), "Status code should be kept!");
    }
}