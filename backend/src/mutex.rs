//! 共用的 `Mutex` 輔助函式。

use std::sync::{Mutex, MutexGuard, PoisonError};

/// 持有鎖的一方 panic 也照常取得：呼叫端保護的資料（回覆片段、近期 request log）沒有需要維持的不變量。
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}
