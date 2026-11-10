//! 登入請求的頻率限制（S-08.2、#295）：每個來源 IP 一次最多連續 `burst` 個，之後每隔 `replenish` 回補一個。
//!
//! Workers 版不用 `tower_governor`（依賴 Tokio 的計時器），改成同樣演算法（token bucket）的 axum middleware。
//! 計數存在 isolate 的記憶體：Workers 會在多個 isolate 執行，各自計算，所以這只是第一道關卡；
//! 跨 isolate 的保護仍是資料庫裡的登入失敗鎖定（`login_failures`）。

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    extract::Request,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::MethodRouter,
};

use crate::{AppState, client_ip, error::ApiError, mutex::lock, rt::Instant};

/// 預設值（暫定，待需求方確認）：連續 60 個，之後每 3 秒回補 1 個。
pub const DEFAULT_BURST: u32 = 60;
pub const DEFAULT_REPLENISH: Duration = Duration::from_secs(3);
pub const BURST: u32 = DEFAULT_BURST;
/// 超過這麼多個來源時清掉已回滿的紀錄，避免記憶體無限成長。
const SWEEP_ABOVE: usize = 4096;

pub(crate) const TOO_MANY_ATTEMPTS: ApiError = ApiError::new(
    axum::http::StatusCode::TOO_MANY_REQUESTS,
    "too_many_attempts",
    "嘗試次數過多，請稍後再試",
);

struct Bucket {
    tokens: f64,
    updated: Instant,
}

struct Limiter {
    burst: f64,
    replenish: Duration,
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl Limiter {
    /// 取一個 token；不夠時回傳要等多久。
    fn take(&self, key: &str, now: Instant) -> Result<(), Duration> {
        let per_sec = 1.0 / self.replenish.as_secs_f64();
        let mut buckets = lock(&self.buckets);
        if buckets.len() > SWEEP_ABOVE {
            let burst = self.burst;
            buckets.retain(|_, b| {
                b.tokens + now.saturating_duration_since(b.updated).as_secs_f64() * per_sec < burst
            });
        }
        let bucket = buckets.entry(key.to_string()).or_insert(Bucket {
            tokens: self.burst,
            updated: now,
        });
        let refill = now.saturating_duration_since(bucket.updated).as_secs_f64() * per_sec;
        bucket.tokens = (bucket.tokens + refill).min(self.burst);
        bucket.updated = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            Err(Duration::from_secs_f64((1.0 - bucket.tokens) / per_sec))
        }
    }
}

pub fn apply(
    route: MethodRouter<AppState>,
    _trusted_proxies: Vec<client_ip::Cidr>,
    burst: u32,
    replenish: Duration,
) -> MethodRouter<AppState> {
    let limiter = Arc::new(Limiter {
        burst: burst as f64,
        replenish,
        buckets: Mutex::default(),
    });
    route.layer(middleware::from_fn(move |req: Request, next: Next| {
        let limiter = limiter.clone();
        async move {
            let key = client_ip::key_from_headers(req.headers());
            match limiter.take(&key, Instant::now()) {
                Ok(()) => next.run(req).await,
                Err(wait) => too_many(wait),
            }
        }
    }))
}

fn too_many(wait: Duration) -> Response {
    let mut response = TOO_MANY_ATTEMPTS.into_response();
    if let Ok(v) = axum::http::HeaderValue::from_str(&wait.as_secs().max(1).to_string()) {
        response.headers_mut().insert("retry-after", v);
    }
    response
}
