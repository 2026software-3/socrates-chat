//! F-19.1：點數帳本與防重複入帳（S-07.2）。
//!
//! 帳本只新增；同一事件（使用者、來源類型、來源 ID）只入帳一次，由資料庫的 UNIQUE 約束保證。
//! 入帳函式接受連線而非連線池，讓呼叫端把入帳與觸發它的事件放在同一個交易內（S-07.2 2.2）。

use std::num::{NonZeroI32, NonZeroU32};

use axum::{Json, Router, extract::State, routing::get};
use serde::Serialize;
use sqlx::PgConnection;
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
///
/// 欄位私有，只能透過下面的建構函式產生，讓 `delta`、`source` 與 `unit_price` 的組合必定符合
/// 資料庫的 CHECK 約束（S-07.2 2.1）。組合錯誤不會等到寫入時才以 `sqlx::Error` 回來。
#[derive(Clone, Debug)]
pub struct Entry<'a> {
    user_id: Uuid,
    /// 正為入帳、負為扣除
    delta: i32,
    reason: &'a str,
    source: Source,
    source_id: &'a str,
    /// 只有扣除時記錄當下價格
    unit_price: Option<i32>,
}

impl<'a> Entry<'a> {
    /// 入帳：完成對話、解鎖成就、任務領取。delta 為正，不記錄價格。
    ///
    /// # Panics
    ///
    /// `source` 不是入帳來源（`Conversation`、`Achievement`、`Task`）時 panic；扣點請用 [`Entry::purchase`]，
    /// 調整請用 [`Entry::adjustment`]。
    pub fn award(
        user_id: Uuid,
        points: NonZeroU32,
        reason: &'a str,
        source: Source,
        source_id: &'a str,
    ) -> Self {
        assert!(
            matches!(
                source,
                Source::Conversation | Source::Achievement | Source::Task
            ),
            "award 只接受入帳來源，收到 {}",
            source.as_str()
        );
        Self {
            user_id,
            delta: to_delta(points.get()),
            reason,
            source,
            source_id,
            unit_price: None,
        }
    }

    /// 部件解鎖／兌換扣點。扣的點數等於當下價格；免費部件（`price` 為 0）寫入 `delta = 0`。
    pub fn purchase(user_id: Uuid, reason: &'a str, part_id: &'a str, price: u32) -> Self {
        let price = to_delta(price);
        Self {
            user_id,
            delta: -price,
            reason,
            source: Source::Purchase,
            source_id: part_id,
            unit_price: Some(price),
        }
    }

    /// 管理者手動調整。delta 可正可負（S-07.2 2.4），不記錄價格。
    pub fn adjustment(
        user_id: Uuid,
        delta: NonZeroI32,
        reason: &'a str,
        adjustment_id: &'a str,
    ) -> Self {
        Self {
            user_id,
            delta: delta.get(),
            reason,
            source: Source::AdminAdjustment,
            source_id: adjustment_id,
            unit_price: None,
        }
    }
}

fn to_delta(points: u32) -> i32 {
    i32::try_from(points).expect("點數超出 i32 範圍")
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

/// 學生看到的點數：只有餘額，不提供帳本明細。
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct Points {
    /// 帳本 delta 的加總
    balance: i64,
}

/// 學生查看自己的餘額。沒有查詢他人點數的路徑，教師與管理者也不能讀（S-07.2 2.5）。
#[utoipa::path(
    get,
    path = "/api/points",
    tag = "points",
    summary = "我的點數餘額",
    responses((status = 200, description = "餘額", body = Points))
)]
pub(crate) async fn mine(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
) -> Result<Json<Points>, ApiError> {
    let mut conn = state.pool.acquire().await?;
    let balance = balance(&mut conn, cu.user.id).await?;
    Ok(Json(Points { balance }))
}
