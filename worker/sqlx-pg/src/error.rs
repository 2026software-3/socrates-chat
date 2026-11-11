//! 錯誤型別：保留後端用到的 SQLx 錯誤介面（`RowNotFound`、`Database`、SQLSTATE、約束名稱）。
//! SQLSTATE 與約束名稱直接取自 PostgreSQL 的錯誤回應，與後端用 SQLx 時相同。

use std::borrow::Cow;

#[derive(Debug)]
pub enum Error {
    RowNotFound,
    Database(Box<PgDatabaseError>),
    /// 查回的資料無法轉成 Rust 型別。
    Decode(String),
    /// 連線或協定層的錯誤（不是 SQL 錯誤）。
    Driver(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 不帶原始訊息：可能含資料內容（與後端相同，不得寫進日誌或回應）
        match self {
            Error::RowNotFound => write!(f, "no rows returned"),
            Error::Database(e) => write!(f, "database error {}", e.code),
            Error::Decode(_) => write!(f, "decode error"),
            Error::Driver(_) => write!(f, "driver error"),
        }
    }
}

impl std::error::Error for Error {}

/// 後端以 `as_database_error()` 取得的介面。
pub trait DatabaseError: std::fmt::Debug + Send + Sync {
    fn code(&self) -> Option<Cow<'_, str>>;
    fn constraint(&self) -> Option<&str>;
    fn message(&self) -> &str;
    fn is_unique_violation(&self) -> bool {
        self.code().as_deref() == Some("23505")
    }
    fn is_foreign_key_violation(&self) -> bool {
        self.code().as_deref() == Some("23503")
    }
    fn is_check_violation(&self) -> bool {
        self.code().as_deref() == Some("23514")
    }
}

#[derive(Debug)]
pub struct PgDatabaseError {
    /// SQLSTATE（`23505` 等）
    code: String,
    constraint: Option<String>,
    /// 原始訊息。可能含資料內容，**不得寫進日誌或回應**。
    message: String,
}

impl DatabaseError for PgDatabaseError {
    fn code(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(&self.code))
    }
    fn constraint(&self) -> Option<&str> {
        self.constraint.as_deref()
    }
    fn message(&self) -> &str {
        &self.message
    }
}

/// 不必 import trait 就能呼叫（後端以 `sqlx::Error::Database(e) if e.constraint() == …` 比對）。
impl PgDatabaseError {
    pub fn constraint(&self) -> Option<&str> {
        self.constraint.as_deref()
    }
    pub fn code(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(&self.code))
    }
    pub fn is_unique_violation(&self) -> bool {
        self.code == "23505"
    }
    pub fn is_foreign_key_violation(&self) -> bool {
        self.code == "23503"
    }
    pub fn is_check_violation(&self) -> bool {
        self.code == "23514"
    }
}

impl Error {
    pub fn as_database_error(&self) -> Option<&dyn DatabaseError> {
        match self {
            Error::Database(e) => Some(e.as_ref()),
            _ => None,
        }
    }

    pub(crate) fn from_pg(e: tokio_postgres::Error) -> Self {
        match e.as_db_error() {
            Some(db) => Error::Database(Box::new(PgDatabaseError {
                code: db.code().code().to_string(),
                constraint: db.constraint().map(str::to_string),
                message: db.message().to_string(),
            })),
            None => Error::Driver(e.to_string()),
        }
    }
}
