//! 每人的滑動時間窗額度：教師「翻譯總結」的頻率限制，以及每位使用者的 AI 用量上限（`ai_usage`）共用。
//!
//! 每次按下都會呼叫一次 AI；沒有限制的話，重複點擊或腳本會讓 AI 費用無上限。
//! 只計入真的呼叫 AI 且得到結果的請求（失敗的會退回額度）。
//! 計數放在記憶體：重啟後歸零，多個實例各自計算（部署方式尚未決定，S-09），所以實際上限是每個實例各自的額度。

use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};

use uuid::Uuid;

use crate::mutex::lock;

/// 每個教師在一個時間窗內最多可呼叫幾次 AI 翻譯。暫定值，待需求方確認。
pub const TRANSLATION_MAX: usize = 30;
/// 計算翻譯額度的時間窗。暫定值，待需求方確認。
pub const TRANSLATION_WINDOW: Duration = Duration::from_secs(600);
/// 記錄的使用者數超過這個數字時，順手清掉已經沒有有效記錄的人，避免記憶體無限成長。
const SWEEP_ABOVE: usize = 1024;

/// 某位使用者目前的用量。
#[derive(Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub used: usize,
    pub max: usize,
    pub window: Duration,
    pub resets_in: Option<Duration>,
}

pub struct UserLimiter {
    max: usize,
    window: Duration,
    hits: Mutex<HashMap<Uuid, VecDeque<Instant>>>,
}

impl Default for UserLimiter {
    fn default() -> Self {
        Self::new(TRANSLATION_MAX, TRANSLATION_WINDOW)
    }
}

impl UserLimiter {
    pub fn new(max: usize, window: Duration) -> Self {
        Self {
            max,
            window,
            hits: Mutex::default(),
        }
    }

    /// 還有額度就記一次並回傳那筆記錄（之後可交給 `refund`）；額度用完回傳 `None`（不記錄）。
    pub fn try_acquire(&self, user: Uuid) -> Option<Instant> {
        let now = Instant::now();
        self.try_acquire_at(user, now).then_some(now)
    }

    /// 退回 `try_acquire` 給的那一筆額度（翻譯失敗時：沒有得到譯文，不該讓重試把額度燒光）。
    /// 退的是自己那一筆，不是該使用者最近一筆：同一人的請求重疊時，別的請求的額度不受影響。
    /// 該筆已經過期被清掉就什麼都不做。
    pub fn refund(&self, user: Uuid, slot: Instant) {
        if let Some(queue) = lock(&self.hits).get_mut(&user)
            && let Some(i) = queue.iter().position(|t| *t == slot)
        {
            queue.remove(i);
        }
    }

    /// 目前時間窗內已用的次數，以及最早那一筆還要多久才過期（也就是多久後會空出一個額度）；沒有記錄時為 `None`。
    pub fn snapshot(&self, user: Uuid) -> Snapshot {
        self.snapshot_at(user, Instant::now())
    }

    fn snapshot_at(&self, user: Uuid, now: Instant) -> Snapshot {
        let hits = lock(&self.hits);
        let live = |t: &&Instant| now.saturating_duration_since(**t) < self.window;
        let queue = hits.get(&user);
        let used = queue.map_or(0, |q| q.iter().filter(live).count());
        let resets_in = queue.and_then(|q| q.iter().find(live)).map(|oldest| {
            self.window
                .saturating_sub(now.saturating_duration_since(*oldest))
        });
        Snapshot {
            used,
            max: self.max,
            window: self.window,
            resets_in,
        }
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
        let limiter = UserLimiter::new(2, Duration::from_secs(60));
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
    fn snapshot_reports_live_usage_and_when_the_oldest_slot_frees() {
        let limiter = UserLimiter::new(3, Duration::from_secs(60));
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let t0 = Instant::now();
        assert_eq!(limiter.snapshot_at(a, t0).used, 0);
        assert_eq!(limiter.snapshot_at(a, t0).resets_in, None);
        assert!(limiter.try_acquire_at(a, t0));
        assert!(limiter.try_acquire_at(a, t0 + Duration::from_secs(20)));
        let s = limiter.snapshot_at(a, t0 + Duration::from_secs(30));
        assert_eq!((s.used, s.max), (2, 3));
        assert_eq!(s.resets_in, Some(Duration::from_secs(30)));
        // 最早那筆過期後只剩一筆
        let s = limiter.snapshot_at(a, t0 + Duration::from_secs(70));
        assert_eq!(s.used, 1);
        assert_eq!(s.resets_in, Some(Duration::from_secs(10)));
        assert_eq!(limiter.snapshot_at(b, t0).used, 0, "別人不受影響");
    }

    #[test]
    fn refund_gives_back_the_slot_only_for_that_user() {
        let limiter = UserLimiter::new(1, Duration::from_secs(60));
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let now = Instant::now();
        assert!(limiter.try_acquire_at(a, now));
        assert!(limiter.try_acquire_at(b, now));
        limiter.refund(a, now);
        assert!(limiter.try_acquire_at(a, now));
        assert!(!limiter.try_acquire_at(b, now), "b was not refunded");
        limiter.refund(Uuid::new_v4(), now); // 沒有記錄的人：什麼都不發生
    }

    #[test]
    fn refund_with_overlapping_requests_keeps_the_other_requests_slot() {
        let limiter = UserLimiter::new(2, Duration::from_secs(60));
        let user = Uuid::new_v4();
        let t0 = Instant::now();
        // 請求 A 在 t0 取得額度，請求 B 在 t0+30 取得；A 還沒結束時 B 已經開始
        assert!(limiter.try_acquire_at(user, t0));
        assert!(limiter.try_acquire_at(user, t0 + Duration::from_secs(30)));
        // A 失敗，退回的必須是 A 自己的額度；B 的額度仍要算到 t0+90 才過期
        limiter.refund(user, t0);
        assert!(limiter.try_acquire_at(user, t0 + Duration::from_secs(61)));
        assert!(
            !limiter.try_acquire_at(user, t0 + Duration::from_secs(62)),
            "B (acquired at t0+30) still counts, so only one slot is free at t0+62"
        );
    }

    #[test]
    fn limits_each_user_separately() {
        let limiter = UserLimiter::new(1, Duration::from_secs(60));
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let now = Instant::now();
        assert!(limiter.try_acquire_at(a, now));
        assert!(!limiter.try_acquire_at(a, now));
        assert!(limiter.try_acquire_at(b, now));
    }
}
