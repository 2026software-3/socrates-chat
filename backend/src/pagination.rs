//! 共用的 keyset 分頁契約（#284）：limit 1–200，預設 50，cursor 為不透明字串。
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::error::ApiError;

#[derive(Default, Deserialize)]
pub struct Pagination {
    limit: Option<String>,
    cursor: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

impl Pagination {
    pub fn limit(&self) -> Result<usize, ApiError> {
        match self.limit.as_deref() {
            None => Ok(50),
            Some(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => s
                .parse::<usize>()
                .ok()
                .filter(|n| (1..=200).contains(n))
                .ok_or_else(|| ApiError::bad_request("invalid_limit", "分頁大小不正確")),
            Some(_) => Err(ApiError::bad_request("invalid_limit", "分頁大小不正確")),
        }
    }

    pub fn cursor<T: DeserializeOwned>(&self, resource: &str) -> Result<Option<T>, ApiError> {
        let Some(raw) = &self.cursor else {
            return Ok(None);
        };
        let invalid = || ApiError::bad_request("invalid_cursor", "分頁游標不正確");
        if raw.len() > 4096 {
            return Err(invalid());
        }
        let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid())?;
        let (version, kind, key): (u8, String, T) =
            serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if version != 1 || kind != resource {
            return Err(invalid());
        }
        Ok(Some(key))
    }
}

/// 查詢 limit + 1 筆，僅在還有下一頁時，以本頁最後一筆產生游標。
pub fn page<T, K: Serialize>(
    mut rows: Vec<T>,
    limit: usize,
    resource: &str,
    key: impl FnOnce(&T) -> K,
) -> Result<Page<T>, ApiError> {
    let more = rows.len() > limit;
    rows.truncate(limit);
    let next_cursor = if more {
        let last = rows.last().ok_or_else(ApiError::internal)?;
        let bytes =
            serde_json::to_vec(&(1u8, resource, key(last))).map_err(|_| ApiError::internal())?;
        Some(URL_SAFE_NO_PAD.encode(bytes))
    } else {
        None
    };
    Ok(Page {
        items: rows,
        next_cursor,
    })
}
