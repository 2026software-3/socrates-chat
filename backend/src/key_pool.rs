//! API key 池：把請求分散到多把 key，避免單一 key 撞到頻率限制。
//!
//! 與供應商無關，只負責「這次用哪把 key」與「哪把暫時／永久不能用」。冷卻狀態只存在記憶體，
//! 後端是單一行程，重啟後清空即可。key 不得出現在日誌、錯誤或 `Debug` 輸出中。

use std::{
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

/// 收到 429 但沒有 `Retry-After` 時的冷卻時間。
pub const DEFAULT_COOLDOWN: Duration = Duration::from_secs(60);
/// `Retry-After` 的上限，避免對方給出離譜的值讓 key 長期不可用。
const MAX_COOLDOWN: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, Copy)]
enum State {
    Ready,
    CoolingUntil(Instant),
    Disabled,
}

struct Slot {
    key: String,
    state: Mutex<State>,
}

pub struct KeyPool {
    slots: Vec<Slot>,
    next: AtomicUsize,
    default_cooldown: Duration,
}

/// 取出的一把 key。`index` 用來回報結果；日誌只記序號，不記 key。
#[derive(Clone)]
pub struct Lease {
    pub index: usize,
    pub key: String,
}

impl std::fmt::Debug for Lease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Lease(key#{})", self.index + 1)
    }
}

impl std::fmt::Debug for KeyPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "KeyPool({} keys)", self.slots.len())
    }
}

impl KeyPool {
    /// 沒有任何 key 時回傳 `None`。
    pub fn new(keys: Vec<String>) -> Option<Self> {
        if keys.is_empty() {
            return None;
        }
        Some(Self {
            slots: keys
                .into_iter()
                .map(|key| Slot {
                    key,
                    state: Mutex::new(State::Ready),
                })
                .collect(),
            next: AtomicUsize::new(0),
            default_cooldown: DEFAULT_COOLDOWN,
        })
    }

    pub fn with_default_cooldown(mut self, d: Duration) -> Self {
        self.default_cooldown = d;
        self
    }

    pub fn default_cooldown(&self) -> Duration {
        self.default_cooldown
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// 輪詢取下一把可用的 key，跳過冷卻中與已停用的；全部不可用時回傳 `None`。
    pub fn acquire(&self) -> Option<Lease> {
        let n = self.slots.len();
        let start = self.next.fetch_add(1, Ordering::Relaxed);
        let now = Instant::now();
        for offset in 0..n {
            let index = (start + offset) % n;
            let slot = &self.slots[index];
            let mut state = slot.state.lock().unwrap_or_else(|e| e.into_inner());
            match *state {
                State::Ready => {}
                State::CoolingUntil(t) if t <= now => *state = State::Ready,
                _ => continue,
            }
            // 下次從這把的下一把開始，讓連續的請求輪流使用
            self.next.store(index + 1, Ordering::Relaxed);
            return Some(Lease {
                index,
                key: slot.key.clone(),
            });
        }
        None
    }

    /// 該 key 被限流：依 `Retry-After`（沒有就用預設值）冷卻。已停用的 key 不會因此復活。
    pub fn mark_rate_limited(&self, index: usize, retry_after: Option<Duration>) {
        let cooldown = retry_after
            .unwrap_or(self.default_cooldown)
            .min(MAX_COOLDOWN);
        if let Some(slot) = self.slots.get(index) {
            let mut state = slot.state.lock().unwrap_or_else(|e| e.into_inner());
            if !matches!(*state, State::Disabled) {
                *state = State::CoolingUntil(Instant::now() + cooldown);
            }
        }
        tracing::warn!(
            key = index + 1,
            cooldown_secs = cooldown.as_secs(),
            "AI key rate limited"
        );
    }

    /// 該 key 被拒絕（無效、額度用盡或無權限）：停用到重啟為止。
    pub fn mark_invalid(&self, index: usize) {
        if let Some(slot) = self.slots.get(index) {
            *slot.state.lock().unwrap_or_else(|e| e.into_inner()) = State::Disabled;
        }
        tracing::error!(key = index + 1, "AI key rejected and disabled");
    }

    /// 依錯誤狀態碼更新該 key 的狀態，回傳換一把 key 重試是否有意義：
    /// 429 冷卻、401/402/403 停用、5xx 與 key 無關；其他狀態（例如 400）換 key 也沒用。
    pub fn report_failure(&self, index: usize, status: u16, retry_after: Option<Duration>) -> bool {
        match status {
            429 => self.mark_rate_limited(index, retry_after),
            401..=403 => self.mark_invalid(index),
            500..=599 => {}
            _ => return false,
        }
        true
    }

    /// 尚未停用的 key 數（包含冷卻中的）。
    pub fn usable_count(&self) -> usize {
        self.slots
            .iter()
            .filter(|s| {
                !matches!(
                    *s.state.lock().unwrap_or_else(|e| e.into_inner()),
                    State::Disabled
                )
            })
            .count()
    }
}

/// 解析 `Retry-After` 標頭；只認秒數，HTTP-date 或無法解析時視為沒有。
pub fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

/// 解析金鑰清單檔：一行一把，忽略空行與 `#` 開頭的註解行，去除前後空白。
pub fn parse_key_lines(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect()
}

/// 合併多個來源的 key：保留第一次出現的順序並去重。
pub fn merge_keys(sources: Vec<Vec<String>>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for k in sources.into_iter().flatten() {
        if !out.contains(&k) {
            out.push(k);
        }
    }
    out
}
