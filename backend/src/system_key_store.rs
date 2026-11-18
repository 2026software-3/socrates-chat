//! 系統 AI 的 key 與設定的資料存取，與 HTTP 無關（管理者在介面管理，見 `system_ai`）。
//!
//! 結構與 `teacher_key_store` 相同，但沒有擁有者：只有管理者能管理，且全部是系統用途。
//! 任何回傳型別的 `Debug` 都不含 key。

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{
    teacher_key_crypto::{KeyCipher, fingerprint, mask},
    teacher_key_store::{AddError, KeyStatus, UsableKey},
};

/// 一筆系統 key 的公開資訊（不含密文與明文）。
#[derive(Debug, Clone, FromRow)]
pub struct SystemKey {
    pub id: Uuid,
    pub provider: String,
    pub mask: String,
    #[sqlx(try_from = "String")]
    pub status: StatusText,
    pub status_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 資料庫的狀態字串轉成 `KeyStatus`。
#[derive(Debug, Clone, Copy)]
pub struct StatusText(pub KeyStatus);

impl From<String> for StatusText {
    fn from(s: String) -> Self {
        Self(KeyStatus::parse(&s))
    }
}

macro_rules! columns {
    () => {
        "id, provider, mask, status, status_at, created_at, updated_at"
    };
}

/// 同一把 key 已被系統或任何教師綁定。
async fn taken(pool: &PgPool, fp: &[u8]) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM teacher_api_keys WHERE fingerprint = $1)
             OR EXISTS (SELECT 1 FROM system_api_keys WHERE fingerprint = $1)",
    )
    .bind(fp)
    .fetch_one(pool)
    .await
}

pub async fn add(
    pool: &PgPool,
    cipher: &KeyCipher,
    actor: Uuid,
    provider: &str,
    key: &str,
) -> Result<SystemKey, AddError> {
    let fp = fingerprint(key);
    if taken(pool, &fp).await? {
        return Err(AddError::Duplicate);
    }
    let sealed = cipher.encrypt(key);
    sqlx::query_as(concat!(
        "INSERT INTO system_api_keys (provider, ciphertext, encryption_key_id, mask, fingerprint, created_by)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING ",
        columns!()
    ))
    .bind(provider)
    .bind(sealed.ciphertext)
    .bind(sealed.key_id)
    .bind(mask(key))
    .bind(fp)
    .bind(actor)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        if e.as_database_error().is_some_and(|d| {
            d.is_unique_violation() && d.constraint() == Some("system_api_keys_fingerprint_key")
        }) {
            AddError::Duplicate
        } else {
            AddError::Db(e)
        }
    })
}

pub async fn list(pool: &PgPool) -> Result<Vec<SystemKey>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT ",
        columns!(),
        " FROM system_api_keys ORDER BY created_at, id"
    ))
    .fetch_all(pool)
    .await
}

/// 更換同一筆項目的 key：`id`、供應商與建立時間不變，狀態重設為可用。不存在回 `Ok(None)`。
pub async fn replace(
    pool: &PgPool,
    cipher: &KeyCipher,
    id: Uuid,
    key: &str,
) -> Result<Option<SystemKey>, AddError> {
    let fp = fingerprint(key);
    // 換成自己原本那把不算重複
    let other: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM teacher_api_keys WHERE fingerprint = $1)
             OR EXISTS (SELECT 1 FROM system_api_keys WHERE fingerprint = $1 AND id <> $2)",
    )
    .bind(&fp)
    .bind(id)
    .fetch_one(pool)
    .await?;
    if other {
        return Err(AddError::Duplicate);
    }
    let sealed = cipher.encrypt(key);
    Ok(sqlx::query_as(concat!(
        "UPDATE system_api_keys
            SET ciphertext = $2, encryption_key_id = $3, mask = $4, fingerprint = $5,
                status = 'ok', status_at = NULL, retry_after_secs = NULL, updated_at = now()
          WHERE id = $1
        RETURNING ",
        columns!()
    ))
    .bind(id)
    .bind(sealed.ciphertext)
    .bind(sealed.key_id)
    .bind(mask(key))
    .bind(fp)
    .fetch_optional(pool)
    .await?)
}

/// 刪除一把 key，回傳被刪項目的供應商；不存在回 `None`。
pub async fn delete(
    db: impl sqlx::PgExecutor<'_>,
    id: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("DELETE FROM system_api_keys WHERE id = $1 RETURNING provider")
        .bind(id)
        .fetch_optional(db)
        .await
}

/// 某供應商的 key（解密後），依建立順序。解密失敗的項目略過並記一筆不含內容的警告。
pub async fn keys_for(
    pool: &PgPool,
    cipher: &KeyCipher,
    provider: &str,
) -> Result<Vec<UsableKey>, sqlx::Error> {
    type Row = (
        Uuid,
        Vec<u8>,
        String,
        String,
        Option<DateTime<Utc>>,
        Option<i32>,
    );
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT id, ciphertext, encryption_key_id, status, status_at, retry_after_secs
           FROM system_api_keys WHERE provider = $1 ORDER BY created_at, id",
    )
    .bind(provider)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, ct, key_id, status, status_at, retry)| {
            match cipher.decrypt(&key_id, &ct) {
                Ok(secret) => Some(UsableKey::from_parts(
                    id,
                    KeyStatus::parse(&status),
                    status_at,
                    retry.map(|s| std::time::Duration::from_secs(s.unsigned_abs().into())),
                    secret,
                )),
                Err(_) => {
                    tracing::warn!(key_id = %id, "system api key could not be decrypted");
                    None
                }
            }
        })
        .collect())
}

/// 取出一把 key（解密後）與其供應商；不存在或解密失敗回 `None`。
pub async fn get_secret(
    pool: &PgPool,
    cipher: &KeyCipher,
    id: Uuid,
) -> Result<Option<(String, UsableKey)>, sqlx::Error> {
    let row: Option<(String, Vec<u8>, String, String)> = sqlx::query_as(
        "SELECT provider, ciphertext, encryption_key_id, status FROM system_api_keys WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|(provider, ct, key_id, status)| {
        cipher.decrypt(&key_id, &ct).ok().map(|secret| {
            (
                provider,
                UsableKey::from_parts(id, KeyStatus::parse(&status), None, None, secret),
            )
        })
    }))
}

pub async fn exists(pool: &PgPool, id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM system_api_keys WHERE id = $1)")
        .bind(id)
        .fetch_one(pool)
        .await
}

/// 以實際用過的 key 記錄狀態：只有那筆項目的 key 還是同一把（指紋相同）才寫入；沒寫入回 `false`。
pub async fn set_status_after(
    pool: &PgPool,
    key: &UsableKey,
    status: KeyStatus,
    retry_after: Option<std::time::Duration>,
) -> Result<bool, sqlx::Error> {
    let secs = retry_after
        .filter(|_| status != KeyStatus::Ok)
        .map(|d| i32::try_from(d.as_secs()).unwrap_or(i32::MAX))
        .filter(|s| *s > 0);
    Ok(sqlx::query(
        "UPDATE system_api_keys
            SET status = $2, status_at = CASE WHEN $2 = 'ok' THEN NULL ELSE now() END,
                retry_after_secs = $4
          WHERE id = $1 AND fingerprint = $3",
    )
    .bind(key.id)
    .bind(status.as_str())
    .bind(fingerprint(key.secret()))
    .bind(secs)
    .execute(pool)
    .await?
    .rows_affected()
        > 0)
}

#[derive(Debug, Clone, FromRow, PartialEq, Eq)]
pub struct Setting {
    pub provider: String,
    pub model: String,
}

pub async fn get_setting(pool: &PgPool) -> Result<Option<Setting>, sqlx::Error> {
    sqlx::query_as("SELECT provider, model FROM system_ai_settings")
        .fetch_optional(pool)
        .await
}

pub async fn set_setting(
    db: impl sqlx::PgExecutor<'_>,
    actor: Uuid,
    provider: &str,
    model: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO system_ai_settings (singleton, provider, model, updated_by)
         VALUES (true, $1, $2, $3)
         ON CONFLICT (singleton) DO UPDATE
            SET provider = $1, model = $2, updated_by = $3, updated_at = now()",
    )
    .bind(provider)
    .bind(model)
    .bind(actor)
    .execute(db)
    .await?;
    Ok(())
}

/// 清除設定（回到主機上的 .apikeylist）；原本有設定才回 `true`。
pub async fn clear_setting(db: impl sqlx::PgExecutor<'_>) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query("DELETE FROM system_ai_settings")
        .execute(db)
        .await?
        .rows_affected()
        > 0)
}

/// 累加一次系統用途的呼叫。`tokens` 為（輸入, 輸出）；供應商沒回報時傳 `None`，只計次數與「未回報」。
pub async fn record_usage(
    pool: &PgPool,
    key_id: Uuid,
    provider: &str,
    model: &str,
    tokens: Option<(i64, i64)>,
) -> Result<(), sqlx::Error> {
    let (input, output, unreported) = match tokens {
        Some((i, o)) => (i, o, 0_i64),
        None => (0, 0, 1),
    };
    sqlx::query(
        "INSERT INTO system_ai_usage
            (key_id, provider, model, day, requests, input_tokens, output_tokens, unreported_requests)
         VALUES ($1, $2, $3, (now() AT TIME ZONE 'Asia/Taipei')::date, 1, $4, $5, $6)
         ON CONFLICT (key_id, provider, model, day) DO UPDATE SET
            requests = system_ai_usage.requests + 1,
            input_tokens = system_ai_usage.input_tokens + EXCLUDED.input_tokens,
            output_tokens = system_ai_usage.output_tokens + EXCLUDED.output_tokens,
            unreported_requests = system_ai_usage.unreported_requests + EXCLUDED.unreported_requests",
    )
    .bind(key_id)
    .bind(provider)
    .bind(model)
    .bind(input)
    .bind(output)
    .bind(unreported)
    .execute(pool)
    .await?;
    Ok(())
}

/// 每把 key 近 `days` 天的用量合計（key 識別碼、請求數、輸入、輸出、未回報）。
pub async fn usage_by_key(
    pool: &PgPool,
    days: i32,
) -> Result<Vec<(Uuid, i64, i64, i64, i64)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT key_id, sum(requests)::bigint, sum(input_tokens)::bigint,
                sum(output_tokens)::bigint, sum(unreported_requests)::bigint
           FROM system_ai_usage
          WHERE day > (now() AT TIME ZONE 'Asia/Taipei')::date - $1
          GROUP BY key_id",
    )
    .bind(days)
    .fetch_all(pool)
    .await
}
