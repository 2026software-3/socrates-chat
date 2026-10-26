//! API key 池：把請求分散到多把 key，避免單一 key 撞到頻率限制。
//!
//! 與供應商無關，只負責「這次用哪把 key」與「哪把暫時不能用」。所有狀態都是記憶體內的冷卻時間，
//! 後端是單一行程，重啟後清空即可；沒有任何 key 是永久停用的，單一 key 的部署也能自行恢復。
//! key 不得出現在日誌、錯誤或 `Debug` 輸出中。

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

/// 收到 429 但沒有 `Retry-After` 時的冷卻時間。
pub const DEFAULT_COOLDOWN: Duration = Duration::from_secs(60);
/// 被拒絕（401/402/403）時的冷卻時間。可能是暫時性的閘道或帳務問題，所以不永久停用，
/// 但也不要太頻繁地重試一把真的有問題的 key。
pub const DEFAULT_AUTH_COOLDOWN: Duration = Duration::from_secs(600);
/// `Retry-After` 的上限，避免對方給出離譜的值讓 key 長期不可用。
const MAX_COOLDOWN: Duration = Duration::from_secs(3600);
/// `Retry-After: 0` 仍冷卻一下，否則重試迴圈會立刻打回同一把。
const MIN_COOLDOWN: Duration = Duration::from_secs(1);

struct Inner {
    /// 每把 key 何時恢復可用；`None` 表示現在就能用。
    cooling_until: Vec<Option<Instant>>,
    next: usize,
}

pub struct KeyPool {
    keys: Vec<String>,
    inner: Mutex<Inner>,
    default_cooldown: Duration,
    auth_cooldown: Duration,
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
        write!(f, "KeyPool({} keys)", self.keys.len())
    }
}

impl KeyPool {
    /// 沒有任何 key 時回傳 `None`。
    pub fn new(keys: Vec<String>) -> Option<Self> {
        if keys.is_empty() {
            return None;
        }
        Some(Self {
            inner: Mutex::new(Inner {
                cooling_until: vec![None; keys.len()],
                next: 0,
            }),
            keys,
            default_cooldown: DEFAULT_COOLDOWN,
            auth_cooldown: DEFAULT_AUTH_COOLDOWN,
        })
    }

    /// 同一批 key、同樣的冷卻設定，但冷卻狀態全新而且互不影響。備援模型用自己的池子，
    /// 這樣主模型的 429（整個模型被限流，不是 key 的問題）不會讓備援模型拿不到 key。
    pub fn fork(&self) -> KeyPool {
        KeyPool {
            keys: self.keys.clone(),
            inner: Mutex::new(Inner {
                cooling_until: vec![None; self.keys.len()],
                next: 0,
            }),
            default_cooldown: self.default_cooldown,
            auth_cooldown: self.auth_cooldown,
        }
    }

    pub fn with_default_cooldown(mut self, d: Duration) -> Self {
        self.default_cooldown = d;
        self
    }

    pub fn with_auth_cooldown(mut self, d: Duration) -> Self {
        self.auth_cooldown = d;
        self
    }

    pub fn default_cooldown(&self) -> Duration {
        self.default_cooldown
    }

    pub fn auth_cooldown(&self) -> Duration {
        self.auth_cooldown
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 輪詢取下一把可用的 key，跳過冷卻中的；全部不可用時回傳 `None`。選取在鎖內完成，
    /// 並發的請求也會平均分配。
    pub fn acquire(&self) -> Option<Lease> {
        let mut inner = self.lock();
        let now = Instant::now();
        let n = self.keys.len();
        for offset in 0..n {
            let index = (inner.next + offset) % n;
            if inner.cooling_until[index].is_none_or(|t| t <= now) {
                inner.cooling_until[index] = None;
                inner.next = (index + 1) % n;
                return Some(Lease {
                    index,
                    key: self.keys[index].clone(),
                });
            }
        }
        None
    }

    /// 最快還要多久才有 key 可用；現在就有可用的 key 時為 `Some(0)`。
    pub fn next_available_in(&self) -> Option<Duration> {
        let inner = self.lock();
        let now = Instant::now();
        inner
            .cooling_until
            .iter()
            .map(|t| t.map_or(Duration::ZERO, |t| t.saturating_duration_since(now)))
            .min()
    }

    fn cool(&self, index: usize, cooldown: Duration) {
        let mut inner = self.lock();
        if let Some(slot) = inner.cooling_until.get_mut(index) {
            // 只延長、不縮短：較長的冷卻（例如被拒絕）不會被後來的短冷卻覆蓋
            let until = Instant::now() + cooldown;
            *slot = Some(slot.map_or(until, |t| t.max(until)));
        }
    }

    /// 該 key 被限流：依 `Retry-After`（沒有就用預設值）冷卻。
    pub fn mark_rate_limited(&self, index: usize, retry_after: Option<Duration>) {
        let cooldown = match retry_after {
            Some(d) if d.is_zero() => MIN_COOLDOWN,
            Some(d) => d.min(MAX_COOLDOWN),
            None => self.default_cooldown,
        };
        self.cool(index, cooldown);
        tracing::warn!(
            key = index + 1,
            cooldown_secs = cooldown.as_secs(),
            "AI key rate limited"
        );
    }

    /// 該 key 被拒絕（無效、額度用盡或無權限）：冷卻一段較長的時間後再試，不永久停用。
    pub fn mark_rejected(&self, index: usize) {
        self.cool(index, self.auth_cooldown);
        tracing::error!(
            key = index + 1,
            cooldown_secs = self.auth_cooldown.as_secs(),
            "AI key rejected, pausing it"
        );
    }

    /// 依錯誤狀態碼更新該 key 的狀態，回傳換一把 key 重試是否有意義：
    /// 429 冷卻、401/402/403 長冷卻、5xx 與 key 無關；其他狀態（例如 400）換 key 也沒用。
    pub fn report_failure(&self, index: usize, status: u16, retry_after: Option<Duration>) -> bool {
        match status {
            429 => self.mark_rate_limited(index, retry_after),
            401..=403 => self.mark_rejected(index),
            500..=599 => {}
            _ => return false,
        }
        true
    }

    /// 現在可用的 key 數（冷卻已結束的也算）。
    pub fn usable_count(&self) -> usize {
        let inner = self.lock();
        let now = Instant::now();
        inner
            .cooling_until
            .iter()
            .filter(|t| t.is_none_or(|t| t <= now))
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
