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
    fn days(self) -> Option<i32> {
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
