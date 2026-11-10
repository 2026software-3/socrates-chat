//! Workers 執行環境下取代 Tokio 時間與背景工作的小工具。
//!
//! - `std::time::Instant` 在 `wasm32-unknown-unknown` 會 panic，改用 JS 時鐘（毫秒）。Workers 的時鐘只在 I/O 時前進
//!   （Spectre 緩解），用來量網路等待、逾時與限流已足夠。
//! - 計時器用 `setTimeout`（`worker::Delay`）。
//! - 背景工作：`spawn` 是 `spawn_local`，只在請求還活著時執行；回應送出後還要繼續的工作用 `spawn_waited`
//!   交給 `ctx.waitUntil`（免費方案在回應結束後最多再給 30 秒）。

use std::{
    future::Future,
    ops::{Add, Sub},
    pin::pin,
    time::Duration,
};

use futures_util::future::{Either, select};
use worker::send::SendFuture;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Instant(f64);

impl Eq for Instant {}

impl Ord for Instant {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl PartialOrd for Instant {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl std::hash::Hash for Instant {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

impl Instant {
    pub fn now() -> Self {
        Instant(js_sys::Date::now())
    }

    pub fn elapsed(&self) -> Duration {
        Instant::now().saturating_duration_since(*self)
    }

    pub fn saturating_duration_since(&self, earlier: Instant) -> Duration {
        Duration::from_secs_f64(((self.0 - earlier.0) / 1000.0).max(0.0))
    }

    pub fn duration_since(&self, earlier: Instant) -> Duration {
        self.saturating_duration_since(earlier)
    }

    pub fn checked_add(&self, d: Duration) -> Option<Instant> {
        Some(*self + d)
    }
}

impl Add<Duration> for Instant {
    type Output = Instant;
    fn add(self, d: Duration) -> Instant {
        Instant(self.0 + d.as_secs_f64() * 1000.0)
    }
}

impl Sub<Duration> for Instant {
    type Output = Instant;
    fn sub(self, d: Duration) -> Instant {
        Instant(self.0 - d.as_secs_f64() * 1000.0)
    }
}

impl Sub<Instant> for Instant {
    type Output = Duration;
    fn sub(self, other: Instant) -> Duration {
        self.saturating_duration_since(other)
    }
}

/// 等待一段時間（`setTimeout`）。
pub fn sleep(d: Duration) -> impl Future<Output = ()> + Send {
    SendFuture::new(worker::Delay::from(d))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Elapsed;

impl std::fmt::Display for Elapsed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("deadline has elapsed")
    }
}

/// `tokio::time::timeout` 的替代。
pub async fn timeout<F: Future>(d: Duration, fut: F) -> Result<F::Output, Elapsed> {
    let fut = pin!(fut);
    let timer = pin!(sleep(d));
    match select(fut, timer).await {
        Either::Left((out, _)) => Ok(out),
        Either::Right(_) => Err(Elapsed),
    }
}

/// `tokio::time::timeout_at` 的替代。
pub async fn timeout_at<F: Future>(deadline: Instant, fut: F) -> Result<F::Output, Elapsed> {
    timeout(deadline.saturating_duration_since(Instant::now()), fut).await
}

/// 在目前的請求內執行一個背景工作（`tokio::spawn` 的替代）。回應送出後不保證繼續執行。
pub fn spawn<F: Future<Output = ()> + 'static>(fut: F) {
    wasm_bindgen_futures::spawn_local(fut);
}

/// 回應送出後仍要完成的工作（AI 回覆、總結）：交給 `ctx.waitUntil`。
/// 沒有 `Context`（例如排程觸發）時退回 `spawn`。
pub fn spawn_waited<F: Future<Output = ()> + 'static>(ctx: Option<&worker::Context>, fut: F) {
    match ctx {
        Some(ctx) => ctx.wait_until(fut),
        None => spawn(fut),
    }
}
