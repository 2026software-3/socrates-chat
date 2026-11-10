//! 錯誤型別：保留後端用到的 SQLx 錯誤介面（`RowNotFound`、`Database`、SQLSTATE、約束名稱）。

use std::borrow::Cow;
use std::sync::OnceLock;

/// SQLite 對「欄位上的唯一索引」只回報欄位清單（`table.col, table.col`），不回報索引名稱。
/// 應用程式啟動時登記欄位清單對應的 PostgreSQL 約束名稱，讓 `constraint()` 與 PostgreSQL 版相同。
static CONSTRAINT_ALIASES: OnceLock<&'static [(&'static str, &'static str)]> = OnceLock::new();

/// 登記 `(欄位清單, 約束名稱)`；只有第一次呼叫有效。
pub fn register_constraint_aliases(aliases: &'static [(&'static str, &'static str)]) {
    let _ = CONSTRAINT_ALIASES.set(aliases);
}

fn alias(columns: &str) -> Option<&'static str> {
    CONSTRAINT_ALIASES
        .get()?
        .iter()
        .find(|(cols, _)| *cols == columns)
        .map(|(_, name)| *name)
}

use wasm_bindgen::{JsCast, JsValue};

#[derive(Debug)]
pub enum Error {
    RowNotFound,
    Database(Box<D1DatabaseError>),
    /// 查回的資料無法轉成 Rust 型別。
    Decode(String),
    /// D1 binding 本身的錯誤（不是 SQL 錯誤）。
    Driver(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::RowNotFound => write!(f, "no rows returned"),
            Error::Database(e) => write!(f, "database error {}", e.code),
            Error::Decode(m) => write!(f, "decode error: {m}"),
            Error::Driver(m) => write!(f, "driver error: {m}"),
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
pub struct D1DatabaseError {
    /// 對應的 PostgreSQL SQLSTATE（`23505` 等），無法分類時是 `D1`。
    code: &'static str,
    /// SQLite 回報的約束：運算式索引是索引名稱，欄位約束是 `table.col, table.col`。
    constraint: Option<String>,
    /// 原始訊息。可能含資料內容，**不得寫進日誌或回應**（與 PostgreSQL 版相同的規則）。
    message: String,
}

impl DatabaseError for D1DatabaseError {
    fn code(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(self.code))
    }
    fn constraint(&self) -> Option<&str> {
        self.constraint.as_deref()
    }
    fn message(&self) -> &str {
        &self.message
    }
}

/// 不必 import trait 就能呼叫（後端以 `sqlx::Error::Database(e) if e.constraint() == …` 比對）。
impl D1DatabaseError {
    pub fn constraint(&self) -> Option<&str> {
        self.constraint.as_deref()
    }
    pub fn code(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(self.code))
    }
}

impl Error {
    pub fn as_database_error(&self) -> Option<&dyn DatabaseError> {
        match self {
            Error::Database(e) => Some(e.as_ref()),
            _ => None,
        }
    }

    pub(crate) fn from_message(message: String) -> Self {
        const KINDS: [(&str, &str); 5] = [
            ("UNIQUE constraint failed", "23505"),
            ("FOREIGN KEY constraint failed", "23503"),
            ("CHECK constraint failed", "23514"),
            ("NOT NULL constraint failed", "23502"),
            ("PRIMARY KEY", "23505"),
        ];
        let Some((kind, code)) = KINDS.iter().find(|(k, _)| message.contains(k)) else {
            if message.contains("SQLITE_ERROR") || message.contains("D1_ERROR") {
                return Error::Database(Box::new(D1DatabaseError {
                    code: "D1",
                    constraint: None,
                    message,
                }));
            }
            return Error::Driver(message);
        };
        let constraint = message.find(kind).and_then(|at| {
            let rest = message[at + kind.len()..].trim_start_matches(':').trim();
            let rest = rest.split(": SQLITE").next().unwrap_or(rest).trim();
            if let Some(name) = rest.strip_prefix("index '") {
                name.split('\'').next().map(str::to_string)
            } else if rest.is_empty() {
                None
            } else {
                Some(alias(rest).map_or_else(|| rest.to_string(), str::to_string))
            }
        });
        Error::Database(Box::new(D1DatabaseError {
            code,
            constraint,
            message,
        }))
    }

    pub(crate) fn from_worker(e: worker::Error) -> Self {
        Self::from_message(e.to_string())
    }

    pub(crate) fn from_js(v: JsValue) -> Self {
        let mut message = match v.dyn_ref::<js_sys::Error>() {
            Some(e) => String::from(e.message()),
            None => v.as_string().unwrap_or_else(|| "unknown D1 error".into()),
        };
        if let Some(cause) = v
            .dyn_ref::<js_sys::Error>()
            .and_then(|e| e.cause().dyn_into::<js_sys::Error>().ok())
        {
            message.push_str(": ");
            message.push_str(&String::from(cause.message()));
        }
        Self::from_message(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_sqlite_constraint_messages() {
        let e = Error::from_message(
            "D1_ERROR: UNIQUE constraint failed: index 'classrooms_normalized_name_unique': SQLITE_CONSTRAINT".into(),
        );
        let db = e.as_database_error().unwrap();
        assert_eq!(db.code().as_deref(), Some("23505"));
        assert_eq!(db.constraint(), Some("classrooms_normalized_name_unique"));

        let e = Error::from_message(
            "UNIQUE constraint failed: conversations.user_id, conversations.activity_id".into(),
        );
        assert_eq!(
            e.as_database_error().unwrap().constraint(),
            Some("conversations.user_id, conversations.activity_id")
        );
    }

    #[test]
    fn maps_column_lists_to_registered_names() {
        register_constraint_aliases(&[(
            "conversations.user_id, conversations.activity_id, conversations.topic_id",
            "conversations_user_activity_uniq",
        )]);
        let e = Error::from_message(
            "D1_ERROR: UNIQUE constraint failed: conversations.user_id, conversations.activity_id, conversations.topic_id: SQLITE_CONSTRAINT".into(),
        );
        assert_eq!(
            e.as_database_error().unwrap().constraint(),
            Some("conversations_user_activity_uniq")
        );
    }
}
