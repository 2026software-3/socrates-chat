//! F-40.8-DB (#602): 教師 key 的 AI 用量累計與彙總，與 HTTP 無關。
//!
//! 只存次數與 token，不存金額、內容或 key。授權由呼叫端（F-40.8-BE）負責。
//! 「日」以 Asia/Taipei 日曆日計（待需求方確認）。

use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    Today,
    Last7Days,
    Last30Days,
    All,
}

impl Range {
    /// 起算日往前幾天（含今天）；`All` 不設限。
    pub(crate) fn days(self) -> Option<i32> {
        match self {
            Self::Today => Some(1),
            Self::Last7Days => Some(7),
            Self::Last30Days => Some(30),
            Self::All => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Totals {
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub unreported_requests: i64,
}

/// 記用量失敗時的日誌；只記 SQLSTATE，不含內容。
///
/// 外鍵錯誤（23503）代表 AI 呼叫途中管理者刪了這把 key（F-40.9）；用量本來就隨 key 一併刪除，
/// 是預期情況，只留 debug，避免被誤判成資料庫問題。其他失敗仍是 warn。
pub fn log_record_failure(e: &sqlx::Error) {
    let class = crate::error::db_error_class(e);
    if class == "23503" {
        tracing::debug!(%class, "AI usage not recorded: key was deleted");
    } else {
        tracing::warn!(%class, "failed to record AI usage");
    }
}

/// 累加一次呼叫。`tokens` 為（輸入, 輸出）；供應商沒回報時傳 `None`，只計請求次數與「未回報」。
pub async fn record(
    pool: &PgPool,
    key_id: Uuid,
    classroom_id: Uuid,
    provider: &str,
    model: &str,
    tokens: Option<(i64, i64)>,
) -> Result<(), sqlx::Error> {
    let (input, output, unreported) = match tokens {
        Some((i, o)) => (i, o, 0_i64),
        None => (0, 0, 1),
    };
    sqlx::query(
        "INSERT INTO teacher_key_usage
            (key_id, classroom_id, provider, model, day, requests, input_tokens, output_tokens, unreported_requests)
         VALUES ($1, $2, $3, $4, (now() AT TIME ZONE 'Asia/Taipei')::date, 1, $5, $6, $7)
         ON CONFLICT (key_id, classroom_id, provider, model, day) DO UPDATE SET
            requests = teacher_key_usage.requests + 1,
            input_tokens = teacher_key_usage.input_tokens + EXCLUDED.input_tokens,
            output_tokens = teacher_key_usage.output_tokens + EXCLUDED.output_tokens,
            unreported_requests = teacher_key_usage.unreported_requests + EXCLUDED.unreported_requests",
    )
    .bind(key_id)
    .bind(classroom_id)
    .bind(provider)
    .bind(model)
    .bind(input)
    .bind(output)
    .bind(unreported)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn totals_for_key(
    pool: &PgPool,
    key_id: Uuid,
    range: Range,
) -> Result<Totals, sqlx::Error> {
    let (requests, input_tokens, output_tokens, unreported_requests): (i64, i64, i64, i64) =
        sqlx::query_as(
            "SELECT COALESCE(SUM(requests), 0)::bigint, COALESCE(SUM(input_tokens), 0)::bigint,
                    COALESCE(SUM(output_tokens), 0)::bigint, COALESCE(SUM(unreported_requests), 0)::bigint
               FROM teacher_key_usage
              WHERE key_id = $1
                AND ($2::int IS NULL OR day > (now() AT TIME ZONE 'Asia/Taipei')::date - $2::int)",
        )
        .bind(key_id)
        .bind(range.days())
        .fetch_one(pool)
        .await?;
    Ok(Totals {
        requests,
        input_tokens,
        output_tokens,
        unreported_requests,
    })
}

pub async fn totals_for_classroom(
    pool: &PgPool,
    classroom_id: Uuid,
    range: Range,
) -> Result<Totals, sqlx::Error> {
    let (requests, input_tokens, output_tokens, unreported_requests): (i64, i64, i64, i64) =
        sqlx::query_as(
            "SELECT COALESCE(SUM(requests), 0)::bigint, COALESCE(SUM(input_tokens), 0)::bigint,
                    COALESCE(SUM(output_tokens), 0)::bigint, COALESCE(SUM(unreported_requests), 0)::bigint
               FROM teacher_key_usage
              WHERE classroom_id = $1
                AND ($2::int IS NULL OR day > (now() AT TIME ZONE 'Asia/Taipei')::date - $2::int)",
        )
        .bind(classroom_id)
        .bind(range.days())
        .fetch_one(pool)
        .await?;
    Ok(Totals {
        requests,
        input_tokens,
        output_tokens,
        unreported_requests,
    })
}

/// 某個（供應商、模型）的累計；備援模型的用量歸在它自己的（供應商、模型）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelUsage {
    pub provider: String,
    pub model: String,
    pub totals: Totals,
}

pub(crate) type ModelRow = (String, String, i64, i64, i64, i64);

pub(crate) fn model_usage(rows: Vec<ModelRow>) -> Vec<ModelUsage> {
    rows.into_iter()
        .map(
            |(provider, model, requests, input_tokens, output_tokens, unreported_requests)| {
                ModelUsage {
                    provider,
                    model,
                    totals: Totals {
                        requests,
                        input_tokens,
                        output_tokens,
                        unreported_requests,
                    },
                }
            },
        )
        .collect()
}

pub async fn by_model_for_key(
    pool: &PgPool,
    key_id: Uuid,
    range: Range,
) -> Result<Vec<ModelUsage>, sqlx::Error> {
    let rows: Vec<ModelRow> = sqlx::query_as(
        "SELECT provider, model, SUM(requests)::bigint, SUM(input_tokens)::bigint,
                SUM(output_tokens)::bigint, SUM(unreported_requests)::bigint
           FROM teacher_key_usage
          WHERE key_id = $1
            AND ($2::int IS NULL OR day > (now() AT TIME ZONE 'Asia/Taipei')::date - $2::int)
          GROUP BY provider, model ORDER BY provider, model",
    )
    .bind(key_id)
    .bind(range.days())
    .fetch_all(pool)
    .await?;
    Ok(model_usage(rows))
}

pub async fn by_model_for_classroom(
    pool: &PgPool,
    classroom_id: Uuid,
    range: Range,
) -> Result<Vec<ModelUsage>, sqlx::Error> {
    let rows: Vec<ModelRow> = sqlx::query_as(
        "SELECT provider, model, SUM(requests)::bigint, SUM(input_tokens)::bigint,
                SUM(output_tokens)::bigint, SUM(unreported_requests)::bigint
           FROM teacher_key_usage
          WHERE classroom_id = $1
            AND ($2::int IS NULL OR day > (now() AT TIME ZONE 'Asia/Taipei')::date - $2::int)
          GROUP BY provider, model ORDER BY provider, model",
    )
    .bind(classroom_id)
    .bind(range.days())
    .fetch_all(pool)
    .await?;
    Ok(model_usage(rows))
}
