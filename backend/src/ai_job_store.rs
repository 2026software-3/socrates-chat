//! 背景 AI 工作的資料表操作（`ai_jobs`，S-20 §3.1）。
//!
//! 登記工作是一次已提交的 INSERT，呼叫端在它之後才 `tokio::spawn`：請求被取消也不會留下沒有紀錄的工作，
//! 也不會重複扣額度。同一個 `scope_key` 未結束的工作只能有一筆（部分唯一索引），再次登記回傳既有的那筆。
//! 狀態只能從 `pending`／`running` 往 `ready`／`failed` 走：被標記為中斷的工作，晚到的結果不會再寫回去。
//! 這裡不存訊息、AI 總結或供應商診斷。

use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

/// 已結束的工作保留幾天後清除（暫定，S-20 §6）。
const RETAIN_DAYS: i32 = 7;

pub enum Registered {
    /// 新登記的工作，呼叫端負責 spawn。
    New(Uuid),
    /// 同範圍已有未結束的工作：不重複啟動、不重複扣額度。
    Existing(Uuid),
}

/// 登記一個工作（`pending`）。
pub async fn register(
    pool: &PgPool,
    owner: Uuid,
    kind: &str,
    scope_key: &str,
    scope: &Value,
    stale_secs: f64,
) -> Result<Registered, sqlx::Error> {
    // 同範圍卡死（worker 已經不在）的舊工作不能永遠擋住新的：超過時限就標為中斷
    sqlx::query(
        "UPDATE ai_jobs SET status = 'failed', error_code = 'interrupted',
                updated_at = now(), finished_at = now()
         WHERE scope_key = $1 AND status IN ('pending', 'running')
           AND updated_at < now() - make_interval(secs => $2)",
    )
    .bind(scope_key)
    .bind(stale_secs)
    .execute(pool)
    .await?;
    // 順手清掉過期的已結束工作（有索引，量很小）
    sqlx::query(
        "DELETE FROM ai_jobs WHERE finished_at IS NOT NULL
           AND finished_at < now() - make_interval(days => $1)",
    )
    .bind(RETAIN_DAYS)
    .execute(pool)
    .await?;
    // 既有的工作剛好在兩次查詢之間結束時，重新嘗試登記
    for _ in 0..3 {
        let inserted: Option<(Uuid,)> = sqlx::query_as(
            "INSERT INTO ai_jobs (kind, owner_user_id, scope_key, scope) VALUES ($1, $2, $3, $4)
             ON CONFLICT (scope_key) WHERE status IN ('pending', 'running') DO NOTHING
             RETURNING id",
        )
        .bind(kind)
        .bind(owner)
        .bind(scope_key)
        .bind(scope)
        .fetch_optional(pool)
        .await?;
        if let Some((id,)) = inserted {
            return Ok(Registered::New(id));
        }
        let existing: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM ai_jobs WHERE scope_key = $1 AND status IN ('pending', 'running')",
        )
        .bind(scope_key)
        .fetch_optional(pool)
        .await?;
        if let Some((id,)) = existing {
            return Ok(Registered::Existing(id));
        }
    }
    Err(sqlx::Error::RowNotFound)
}

/// 工作開始執行。
pub async fn mark_running(pool: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE ai_jobs SET status = 'running', updated_at = now() WHERE id = $1 AND status = 'pending'",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// 工作完成；`result` 只放結果摘要。已被標記結束的工作不會被覆寫。
pub async fn finish_ready(pool: &PgPool, id: Uuid, result: &Value) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE ai_jobs SET status = 'ready', result = $2, updated_at = now(), finished_at = now()
         WHERE id = $1 AND status IN ('pending', 'running')",
    )
    .bind(id)
    .bind(result)
    .execute(pool)
    .await?;
    Ok(())
}

/// 工作失敗；`code` 是固定的錯誤代碼，不含堆疊、金鑰或供應商診斷。
pub async fn finish_failed(pool: &PgPool, id: Uuid, code: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE ai_jobs SET status = 'failed', error_code = $2, updated_at = now(), finished_at = now()
         WHERE id = $1 AND status IN ('pending', 'running')",
    )
    .bind(id)
    .bind(code)
    .execute(pool)
    .await?;
    Ok(())
}

/// 啟動時把超過 `stale_secs` 仍未結束的工作標為失敗（`interrupted`）：它的 worker 已經不在了。
/// 時限內的留給可能仍在執行的實例（例如滾動部署）。
pub async fn recover_orphaned(pool: &PgPool, stale_secs: f64) -> Result<u64, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE ai_jobs SET status = 'failed', error_code = 'interrupted',
                updated_at = now(), finished_at = now()
         WHERE status IN ('pending', 'running')
           AND updated_at < now() - make_interval(secs => $1)",
    )
    .bind(stale_secs)
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}
