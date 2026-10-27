//! 教師「翻譯總結」每人的頻率限制。
//!
//! 翻譯結果不保存，每次按下都會呼叫一次 AI；沒有限制的話，重複點擊或腳本會讓 AI 費用無上限。
//! 只計入真的呼叫 AI 的請求（語言相同直接回傳的不算）。計數放在記憶體，重啟後歸零。

use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};

use uuid::Uuid;

use crate::mutex::lock;

/// 每個教師在一個時間窗內最多可呼叫幾次 AI 翻譯。暫定值，待需求方確認。
pub const DEFAULT_MAX: usize = 30;
/// 計算額度的時間窗。暫定值，待需求方確認。
pub const DEFAULT_WINDOW: Duration = Duration::from_secs(600);
/// 記錄的使用者數超過這個數字時，順手清掉已經沒有有效記錄的人，避免記憶體無限成長。
const SWEEP_ABOVE: usize = 1024;

pub struct TranslationLimiter {
    max: usize,
    window: Duration,
    hits: Mutex<HashMap<Uuid, VecDeque<Instant>>>,
}

impl Default for TranslationLimiter {
    fn default() -> Self {
        Self::new(DEFAULT_MAX, DEFAULT_WINDOW)
    }
}

impl TranslationLimiter {
    pub fn new(max: usize, window: Duration) -> Self {
        Self {
            max,
            window,
            hits: Mutex::default(),
        }
    }

    /// 還有額度就記一次並回傳 `true`；額度用完回傳 `false`（不記錄）。
    pub fn try_acquire(&self, user: Uuid) -> bool {
        self.try_acquire_at(user, Instant::now())
    }

    fn try_acquire_at(&self, user: Uuid, now: Instant) -> bool {
        let expired = |t: &Instant| now.saturating_duration_since(*t) >= self.window;
        let mut hits = lock(&self.hits);
        if hits.len() > SWEEP_ABOVE {
            hits.retain(|_, q| q.back().is_some_and(|t| !expired(t)));
        }
        let queue = hits.entry(user).or_default();
        while queue.front().is_some_and(expired) {
            queue.pop_front();
        }
        if queue.len() >= self.max {
            return false;
        }
        queue.push_back(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_max_per_window_then_recovers() {
        let limiter = TranslationLimiter::new(2, Duration::from_secs(60));
        let user = Uuid::new_v4();
        let t0 = Instant::now();
        assert!(limiter.try_acquire_at(user, t0));
        assert!(limiter.try_acquire_at(user, t0 + Duration::from_secs(1)));
        assert!(!limiter.try_acquire_at(user, t0 + Duration::from_secs(2)));
        // 兩次記錄都滑出時間窗（滿 60 秒就算過期）後，額度完全回復
        assert!(limiter.try_acquire_at(user, t0 + Duration::from_secs(61)));
        assert!(limiter.try_acquire_at(user, t0 + Duration::from_secs(62)));
        assert!(!limiter.try_acquire_at(user, t0 + Duration::from_secs(63)));
        // 只有最早那一次過期時，只回復一個額度
        assert!(limiter.try_acquire_at(user, t0 + Duration::from_secs(121)));
        assert!(!limiter.try_acquire_at(user, t0 + Duration::from_secs(121)));
    }

    #[test]
    fn limits_each_user_separately() {
        let limiter = TranslationLimiter::new(1, Duration::from_secs(60));
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let now = Instant::now();
        assert!(limiter.try_acquire_at(a, now));
        assert!(!limiter.try_acquire_at(a, now));
        assert!(limiter.try_acquire_at(b, now));
    }
}
