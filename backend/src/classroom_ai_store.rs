//! F-40.4-DB (#596): 教室的 AI 對話模型、備援模型、供應商確認與過渡旗標，與 HTTP 無關。
//!
//! 授權與供應商名單驗證在 F-40.4-BE；這裡只守形狀（備援至多三個、供應商與模型非空）。

use sqlx::PgPool;
use uuid::Uuid;

pub const MAX_FALLBACKS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassroomAi {
    pub primary: ModelRef,
    /// 依嘗試順序。
    pub fallbacks: Vec<ModelRef>,
}

#[derive(Debug)]
pub enum SetError {
    TooManyFallbacks,
    /// 供應商不是小寫英數、底線與連字號，或模型空白、過長。
    Invalid,
    Db(sqlx::Error),
}

impl From<sqlx::Error> for SetError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}

fn valid(m: &ModelRef) -> bool {
    !m.provider.is_empty()
        && m.provider.len() <= 64
        && m.provider
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        && !m.model.trim().is_empty()
        && m.model.chars().count() <= 200
}

pub async fn get(pool: &PgPool, classroom: Uuid) -> Result<Option<ClassroomAi>, sqlx::Error> {
    let primary: Option<(String, String)> =
        sqlx::query_as("SELECT provider, model FROM classroom_ai_models WHERE classroom_id = $1")
            .bind(classroom)
            .fetch_optional(pool)
            .await?;
    let Some((provider, model)) = primary else {
        return Ok(None);
    };
    let fallbacks: Vec<(String, String)> = sqlx::query_as(
        "SELECT provider, model FROM classroom_ai_fallbacks WHERE classroom_id = $1 ORDER BY position",
    )
    .bind(classroom)
    .fetch_all(pool)
    .await?;
    Ok(Some(ClassroomAi {
        primary: ModelRef { provider, model },
        fallbacks: fallbacks
            .into_iter()
            .map(|(provider, model)| ModelRef { provider, model })
            .collect(),
    }))
}

/// 設定（或整組取代）教室的主模型與備援，同一個交易。不清除過渡旗標（見 `clear_transitional`）。
pub async fn set(
    pool: &PgPool,
    classroom: Uuid,
    primary: ModelRef,
    fallbacks: Vec<ModelRef>,
) -> Result<(), SetError> {
    if fallbacks.len() > MAX_FALLBACKS {
        return Err(SetError::TooManyFallbacks);
    }
    if !valid(&primary) || !fallbacks.iter().all(valid) {
        return Err(SetError::Invalid);
    }
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO classroom_ai_models (classroom_id, provider, model) VALUES ($1, $2, $3)
         ON CONFLICT (classroom_id) DO UPDATE
            SET provider = EXCLUDED.provider, model = EXCLUDED.model, updated_at = now()",
    )
    .bind(classroom)
    .bind(&primary.provider)
    .bind(&primary.model)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM classroom_ai_fallbacks WHERE classroom_id = $1")
        .bind(classroom)
        .execute(&mut *tx)
        .await?;
    for (i, f) in fallbacks.iter().enumerate() {
        sqlx::query(
            "INSERT INTO classroom_ai_fallbacks (classroom_id, position, provider, model) VALUES ($1, $2, $3, $4)",
        )
        .bind(classroom)
        .bind(i as i16 + 1)
        .bind(&f.provider)
        .bind(&f.model)
        .execute(&mut *tx)
        .await?;
    }
    // 備援清單換了：各場對話記下的備援位置已對不上新清單，一律從主模型重新開始（F-40.6）
    sqlx::query(
        "UPDATE conversations SET ai_fallback_position = 0
          WHERE classroom_id = $1 AND ai_fallback_position <> 0",
    )
    .bind(classroom)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// 記錄教師對（教室、供應商）的確認；第一次回 `true`，已確認過回 `false` 且不覆蓋原紀錄。
pub async fn confirm(
    pool: &PgPool,
    classroom: Uuid,
    provider: &str,
    by: Uuid,
) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query(
        "INSERT INTO classroom_ai_provider_confirmations (classroom_id, provider, confirmed_by)
         VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(classroom)
    .bind(provider)
    .bind(by)
    .execute(pool)
    .await?
    .rows_affected()
        > 0)
}

pub async fn is_confirmed(
    pool: &PgPool,
    classroom: Uuid,
    provider: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM classroom_ai_provider_confirmations WHERE classroom_id = $1 AND provider = $2)",
    )
    .bind(classroom)
    .bind(provider)
    .fetch_one(pool)
    .await
}

pub async fn is_transitional(pool: &PgPool, classroom: Uuid) -> Result<bool, sqlx::Error> {
    Ok(
        sqlx::query_scalar("SELECT ai_transitional FROM classrooms WHERE id = $1")
            .bind(classroom)
            .fetch_optional(pool)
            .await?
            .unwrap_or(false),
    )
}

/// 清除過渡旗標；原本是過渡中才回 `true`。
pub async fn clear_transitional(pool: &PgPool, classroom: Uuid) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query(
        "UPDATE classrooms SET ai_transitional = false WHERE id = $1 AND ai_transitional",
    )
    .bind(classroom)
    .execute(pool)
    .await?
    .rows_affected()
        > 0)
}

/// 已確認過資料流的供應商，依識別碼排序。
pub async fn confirmed_providers(
    pool: &PgPool,
    classroom: Uuid,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT provider FROM classroom_ai_provider_confirmations WHERE classroom_id = $1 ORDER BY provider",
    )
    .bind(classroom)
    .fetch_all(pool)
    .await
}
