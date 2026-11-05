//! `POST /api/auth/login` 依來源 IP 的請求頻率限制（S-08.2、#295）。
//!
//! 這只擋洪水式的請求，不負責猜密碼的防護（那是（信箱, IP）的失敗鎖定）。額度刻意放寬，
//! 因為校園 NAT 後面可能有一整個班的學生共用同一個 IP。計數放在記憶體，重啟後歸零。

use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{extract::ConnectInfo, http::Request, response::IntoResponse, routing::MethodRouter};
use tower_governor::{
    GovernorError, GovernorLayer, governor::GovernorConfigBuilder, key_extractor::KeyExtractor,
};

use crate::{
    AppState,
    client_ip::{self, Cidr},
    error::ApiError,
};

/// 每個來源 IP 一次最多可連續送出幾個登入請求。暫定值，待需求方確認。
/// 刻意不低於一個班的人數（效能目標以 60 人同時使用量測）：校園 NAT 後整班同時登入不能被誤擋。
pub const DEFAULT_BURST: u32 = 60;
/// 額度回補的間隔：每 3 秒回補一個請求（平均每分鐘 20 個，原為每 0.5 秒一個）。
/// 登入每次都要做一次 Argon2 雜湊，持續的洪水主要耗 CPU，所以收緊的是持續速率而不是瞬間額度。暫定值，待需求方確認。
pub const DEFAULT_REPLENISH: Duration = Duration::from_secs(3);
/// 舊名稱，保留給既有測試。
pub const BURST: u32 = DEFAULT_BURST;
/// 清掉閒置來源的記憶體的間隔。
const CLEANUP_EVERY: Duration = Duration::from_secs(60);

/// 登入嘗試過多：IP 頻率限制與（信箱, IP）失敗鎖定共用同一個錯誤碼與格式。
pub(crate) const TOO_MANY_ATTEMPTS: ApiError = ApiError::new(
    axum::http::StatusCode::TOO_MANY_REQUESTS,
    "too_many_attempts",
    "嘗試次數過多，請稍後再試",
);

/// 以來源 IP（`client_ip` 的規則）為鍵；沒有連線資訊時所有請求共用空字串這一個鍵。
#[derive(Clone)]
pub struct ClientIpKey {
    trusted_proxies: Arc<Vec<Cidr>>,
}

impl KeyExtractor for ClientIpKey {
    type Key = String;

    fn extract<T>(&self, req: &Request<T>) -> Result<Self::Key, GovernorError> {
        let peer = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(addr)| addr.ip());
        let forwarded_for = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok());
        Ok(client_ip::client_key(
            peer,
            forwarded_for,
            &self.trusted_proxies,
        ))
    }
}

/// 超過額度時的回應：與信箱鎖定同一個錯誤碼與格式，前端不用改。
fn too_many_requests(error: GovernorError) -> axum::response::Response {
    let mut response = TOO_MANY_ATTEMPTS.into_response();
    if let GovernorError::TooManyRequests {
        headers: Some(headers),
        ..
    } = error
    {
        response.headers_mut().extend(headers);
    }
    response
}

/// 把頻率限制套到登入路由上。
pub fn apply(
    route: MethodRouter<AppState>,
    trusted_proxies: Vec<Cidr>,
    burst: u32,
    replenish: Duration,
) -> MethodRouter<AppState> {
    let config = Arc::new(
        GovernorConfigBuilder::default()
            .period(replenish)
            .burst_size(burst)
            .key_extractor(ClientIpKey {
                trusted_proxies: Arc::new(trusted_proxies),
            })
            .finish()
            .expect("valid rate limit configuration"),
    );
    // 閒置的來源不會自己消失，定期清掉以免記憶體隨不同 IP 無限成長
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        let limiter = config.limiter().clone();
        runtime.spawn(async move {
            loop {
                tokio::time::sleep(CLEANUP_EVERY).await;
                limiter.retain_recent();
            }
        });
    }
    route.layer(GovernorLayer::new(config).error_handler(too_many_requests))
}
