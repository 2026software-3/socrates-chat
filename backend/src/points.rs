//! F-19.1：點數帳本與防重複入帳（S-07.2）。
//!
//! 帳本只新增；同一事件（使用者、來源類型、來源 ID）只入帳一次，由資料庫的 UNIQUE 約束保證。
//! 入帳函式接受連線而非連線池，讓呼叫端把入帳與觸發它的事件放在同一個交易內（S-07.2 2.2）。

use axum::{Json, Router, extract::State, routing::get};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgConnection};
use uuid::Uuid;

use crate::{AppState, auth::RequireStudent, error::ApiError};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/points", get(mine))
}

/// 帳本紀錄的來源類型（S-07.2 2.1）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// 完成對話發點；來源 ID 為對話 ID
    Conversation,
    /// 解鎖成就發點；來源 ID 為成就 ID
    Achievement,
    /// 任務領取；來源 ID 為 `<task_id>:<period_start>`
    Task,
    /// 部件解鎖／兌換扣點；來源 ID 為部件 ID
    Purchase,
    /// 管理者手動調整；來源 ID 為該次調整的唯一 ID
    AdminAdjustment,
}

impl Source {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::Achievement => "achievement",
            Self::Task => "task",
            Self::Purchase => "purchase",
            Self::AdminAdjustment => "admin_adjustment",
        }
    }
}

/// 一筆要寫入的帳本紀錄。
#[derive(Clone, Debug)]
pub struct Entry<'a> {
    pub user_id: Uuid,
    /// 正為入帳、負為扣除
    pub delta: i32,
    pub reason: &'a str,
    pub source: Source,
    pub source_id: &'a str,
    /// 只有扣除時記錄當下價格
    pub unit_price: Option<i32>,
}

/// 寫入一筆帳本紀錄；回傳是否真的寫入。
///
/// 同一事件已入帳時不寫入、也不回錯誤（冪等），並行送出時同樣只有一筆成功。
/// 不檢查餘額：扣點的餘額檢查與鎖定由購買流程負責（S-07.2 2.3）。
pub async fn record(conn: &mut PgConnection, entry: &Entry<'_>) -> Result<bool, sqlx::Error> {
    let r = sqlx::query(
        "INSERT INTO point_ledger (user_id, delta, reason, source_type, source_id, unit_price)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (user_id, source_type, source_id) DO NOTHING",
    )
    .bind(entry.user_id)
    .bind(entry.delta)
    .bind(entry.reason)
    .bind(entry.source.as_str())
    .bind(entry.source_id)
    .bind(entry.unit_price)
    .execute(conn)
    .await?;
    Ok(r.rows_affected() == 1)
}

/// 目前餘額：帳本 delta 的加總。
pub async fn balance(conn: &mut PgConnection, user_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COALESCE(sum(delta), 0)::bigint FROM point_ledger WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_one(conn)
    .await
}

/// 回給學生的一筆帳本紀錄：只有 ID 與數值。
#[derive(Serialize, FromRow)]
pub struct LedgerRow {
    pub delta: i32,
    pub reason: String,
    pub source_type: String,
    pub source_id: String,
    pub unit_price: Option<i32>,
    pub created_at: DateTime<Utc>,
}

/// 使用者的帳本，新的在前。
pub async fn ledger(conn: &mut PgConnection, user_id: Uuid) -> Result<Vec<LedgerRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT delta, reason, source_type, source_id, unit_price, created_at
         FROM point_ledger WHERE user_id = $1
         ORDER BY created_at DESC, id DESC",
    )
    .bind(user_id)
    .fetch_all(conn)
    .await
}

#[derive(Serialize)]
struct Points {
    balance: i64,
    ledger: Vec<LedgerRow>,
}

/// 學生查看自己的餘額與帳本。沒有查詢他人點數的路徑，教師與管理者也不能讀（S-07.2 2.5）。
async fn mine(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
) -> Result<Json<Points>, ApiError> {
    let mut conn = state.pool.acquire().await?;
    let balance = balance(&mut conn, cu.user.id).await?;
    let ledger = ledger(&mut conn, cu.user.id).await?;
    Ok(Json(Points { balance, ledger }))
}
