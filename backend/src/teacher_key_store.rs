//! F-40.1-DB (#591): 教師 API key 的資料存取，與 HTTP 無關。
//!
//! 授權（誰能新增、更換、刪除）由呼叫端（F-40.1-BE）負責；這裡的「依擁有者」函式只是把範圍限制在該教師。
//! 任何回傳型別的 `Debug` 都不含 key。

use std::fmt;

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::teacher_key_crypto::{KeyCipher, fingerprint, mask};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStatus {
    Ok,
    Invalid,
    RateLimited,
    QuotaExhausted,
}

impl KeyStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Invalid => "invalid",
            Self::RateLimited => "rate_limited",
            Self::QuotaExhausted => "quota_exhausted",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "invalid" => Self::Invalid,
            "rate_limited" => Self::RateLimited,
            "quota_exhausted" => Self::QuotaExhausted,
            _ => Self::Ok,
        }
    }
}

/// 一筆 key 的公開資訊（不含密文與明文）。
#[derive(Debug, Clone)]
pub struct TeacherKey {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub provider: String,
    pub mask: String,
    pub status: KeyStatus,
    pub status_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct Row {
    id: Uuid,
    owner_id: Uuid,
    provider: String,
    mask: String,
    status: String,
    status_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<Row> for TeacherKey {
    fn from(r: Row) -> Self {
        Self {
            id: r.id,
            owner_id: r.owner_id,
            provider: r.provider,
            mask: r.mask,
            status: KeyStatus::parse(&r.status),
            status_at: r.status_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

macro_rules! columns {
    () => {
        "id, owner_id, provider, mask, status, status_at, created_at, updated_at"
    };
}

/// 解密後的 key，供路由呼叫供應商用。`Debug` 不印出內容。
pub struct UsableKey {
    pub id: Uuid,
    pub status: KeyStatus,
    secret: String,
}

impl UsableKey {
    pub fn secret(&self) -> &str {
        &self.secret
    }
}

impl fmt::Debug for UsableKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UsableKey({})", self.id)
    }
}

#[derive(Debug)]
pub enum AddError {
    /// 這把 key 已被綁定（不論哪位教師、哪個供應商）。
    Duplicate,
    Db(sqlx::Error),
}

impl From<sqlx::Error> for AddError {
    fn from(e: sqlx::Error) -> Self {
        if e.as_database_error().is_some_and(|d| {
            d.is_unique_violation() && d.constraint() == Some("teacher_api_keys_fingerprint_key")
        }) {
            Self::Duplicate
        } else {
            Self::Db(e)
        }
    }
}

pub async fn add(
    pool: &PgPool,
    cipher: &KeyCipher,
    owner: Uuid,
    provider: &str,
    key: &str,
) -> Result<TeacherKey, AddError> {
    let sealed = cipher.encrypt(key);
    let row: Row = sqlx::query_as(concat!(
        "INSERT INTO teacher_api_keys (owner_id, provider, ciphertext, encryption_key_id, mask, fingerprint)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING ", columns!()
    ))
    .bind(owner)
    .bind(provider)
    .bind(sealed.ciphertext)
    .bind(sealed.key_id)
    .bind(mask(key))
    .bind(fingerprint(key))
    .fetch_one(pool)
    .await?;
    Ok(row.into())
}

pub async fn list_for_owner(pool: &PgPool, owner: Uuid) -> Result<Vec<TeacherKey>, sqlx::Error> {
    let rows: Vec<Row> = sqlx::query_as(concat!(
        "SELECT ",
        columns!(),
        " FROM teacher_api_keys WHERE owner_id = $1 ORDER BY created_at, id"
    ))
    .bind(owner)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// 更換同一筆項目的 key：`id`、供應商與建立時間不變，狀態重設為可用。
/// 不是該擁有者的 key 或不存在時回 `Ok(None)`。
pub async fn replace(
    pool: &PgPool,
    cipher: &KeyCipher,
    owner: Uuid,
    id: Uuid,
    key: &str,
) -> Result<Option<TeacherKey>, AddError> {
    let sealed = cipher.encrypt(key);
    let row: Option<Row> = sqlx::query_as(concat!(
        "UPDATE teacher_api_keys
            SET ciphertext = $3, encryption_key_id = $4, mask = $5, fingerprint = $6,
                status = 'ok', status_at = NULL, updated_at = now()
          WHERE id = $1 AND owner_id = $2
        RETURNING ",
        columns!(),
        ""
    ))
    .bind(id)
    .bind(owner)
    .bind(sealed.ciphertext)
    .bind(sealed.key_id)
    .bind(mask(key))
    .bind(fingerprint(key))
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

/// 刪除自己的 key；回傳是否真的刪了一筆。
pub async fn delete(pool: &PgPool, owner: Uuid, id: Uuid) -> Result<bool, sqlx::Error> {
    Ok(
        sqlx::query("DELETE FROM teacher_api_keys WHERE id = $1 AND owner_id = $2")
            .bind(id)
            .bind(owner)
            .execute(pool)
            .await?
            .rows_affected()
            > 0,
    )
}

/// 管理者刪除任一把 key（F-40.9）：回傳被刪項目的公開資訊，供通知與稽核；不存在回 `None`。
pub async fn delete_any(pool: &PgPool, id: Uuid) -> Result<Option<TeacherKey>, sqlx::Error> {
    let row: Option<Row> = sqlx::query_as(concat!(
        "DELETE FROM teacher_api_keys WHERE id = $1 RETURNING ",
        columns!(),
        ""
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

/// 該教師在某供應商的 key（解密後），依建立順序。解密失敗的項目略過並記一筆不含內容的警告，
/// 不讓一把壞資料拖垮整批。是否使用（狀態、帳號停用）由路由層決定。
pub async fn keys_for(
    pool: &PgPool,
    cipher: &KeyCipher,
    owner: Uuid,
    provider: &str,
) -> Result<Vec<UsableKey>, sqlx::Error> {
    let rows: Vec<(Uuid, Vec<u8>, String, String)> = sqlx::query_as(
        "SELECT id, ciphertext, encryption_key_id, status FROM teacher_api_keys
          WHERE owner_id = $1 AND provider = $2 ORDER BY created_at, id",
    )
    .bind(owner)
    .bind(provider)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(
            |(id, ct, key_id, status)| match cipher.decrypt(&key_id, &ct) {
                Ok(secret) => Some(UsableKey {
                    id,
                    status: KeyStatus::parse(&status),
                    secret,
                }),
                Err(_) => {
                    tracing::warn!(key_id = %id, "teacher api key could not be decrypted");
                    None
                }
            },
        )
        .collect())
}

/// 以實際用過的 key 記錄狀態：只有那筆項目的 key 還是同一把（指紋相同）才寫入。
/// 呼叫進行中教師更換了 key 時，舊 key 的失敗不會蓋到新 key 上；沒寫入回 `false`。
pub async fn set_status_for(
    pool: &PgPool,
    key: &UsableKey,
    status: KeyStatus,
) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query(
        "UPDATE teacher_api_keys
            SET status = $2, status_at = CASE WHEN $2 = 'ok' THEN NULL ELSE now() END
          WHERE id = $1 AND fingerprint = $3",
    )
    .bind(key.id)
    .bind(status.as_str())
    .bind(fingerprint(key.secret()))
    .execute(pool)
    .await?
    .rows_affected()
        > 0)
}

/// 記錄 key 的狀態；`Ok` 會清除狀態時間。key 不存在回 `false`。
pub async fn set_status(pool: &PgPool, id: Uuid, status: KeyStatus) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query(
        "UPDATE teacher_api_keys
            SET status = $2, status_at = CASE WHEN $2 = 'ok' THEN NULL ELSE now() END
          WHERE id = $1",
    )
    .bind(id)
    .bind(status.as_str())
    .execute(pool)
    .await?
    .rows_affected()
        > 0)
}

/// 取出自己的一把 key（解密後）與其供應商；不存在、不是自己的或解密失敗回 `None`。
pub async fn get_secret(
    pool: &PgPool,
    cipher: &KeyCipher,
    owner: Uuid,
    id: Uuid,
) -> Result<Option<(String, UsableKey)>, sqlx::Error> {
    let row: Option<(String, Vec<u8>, String, String)> = sqlx::query_as(
        "SELECT provider, ciphertext, encryption_key_id, status FROM teacher_api_keys
          WHERE id = $1 AND owner_id = $2",
    )
    .bind(id)
    .bind(owner)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|(provider, ct, key_id, status)| {
        cipher.decrypt(&key_id, &ct).ok().map(|secret| {
            (
                provider,
                UsableKey {
                    id,
                    status: KeyStatus::parse(&status),
                    secret,
                },
            )
        })
    }))
}

/// 這把 key 是否屬於該教師。
pub async fn owns(pool: &PgPool, owner: Uuid, id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM teacher_api_keys WHERE id = $1 AND owner_id = $2)",
    )
    .bind(id)
    .bind(owner)
    .fetch_one(pool)
    .await
}
