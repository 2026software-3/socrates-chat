//! 後台首頁的服務健康度（F-39.2 / S-09.3）：`GET /api/admin/health`。

use std::{sync::atomic::Ordering, time::Instant};

use axum::{
    Json, Router,
    extract::{Query, State, rejection::QueryRejection},
    http::{HeaderValue, header},
    routing::get,
};
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    AppState,
    auth::RequireAdmin,
    error::{ApiError, ErrorBody},
    request_stats,
};

const DEFAULT_PERIOD_DAYS: i64 = 1;
const MAX_PERIOD_DAYS: i64 = 30;
const STATS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
const CLEANUP_STALE_AFTER: TimeDelta = TimeDelta::hours(48);
/// p95 超過這個值（毫秒）算降級；分桶的上限是 3000 的下一格是 5000，所以 > 3000 等於「p95 ≥ 3 秒」。
const SLOW_P95_MS: u64 = 3000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Overall {
    Healthy,
    Degraded,
    Unhealthy,
}

#[derive(Serialize, ToSchema)]
struct DatabaseHealth {
    /// `healthy` 或 `unhealthy`
    status: &'static str,
    response_time_ms: Option<u64>,
}

#[derive(Serialize, ToSchema)]
struct PoolHealth {
    size: u32,
    idle: u32,
    max: u32,
}

#[derive(Serialize, ToSchema)]
struct ProcessHealth {
    started_at: DateTime<Utc>,
    uptime_seconds: i64,
    version: String,
}

#[derive(Serialize, ToSchema)]
struct CleanupHealth {
    last_success_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, ToSchema)]
struct RequestHealth {
    total: i64,
    /// 5xx 佔全部請求的比例（0–1）
    error_5xx_ratio: f64,
    /// 耗時分桶估算的 p95（毫秒，分桶上限；超過 10 秒時為 10000）
    p95_ms: u64,
}

#[derive(Serialize, ToSchema)]
struct HealthView {
    status: Overall,
    period_days: i64,
    database: DatabaseHealth,
    services: crate::service_monitor::ServiceSummary,
    pool: PoolHealth,
    process: ProcessHealth,
    maintenance: bool,
    cleanup: CleanupHealth,
    /// 統計讀取失敗時為 `false`，`requests` 為 `null`
    stats_available: bool,
    /// 期間內沒有請求，或統計讀取失敗時為 `null`
    requests: Option<RequestHealth>,
}

#[derive(Deserialize)]
struct HealthQuery {
    period_days: Option<i64>,
}

pub(crate) struct Inputs {
    pub database_up: bool,
    pub peripheral_unhealthy: bool,
    pub requests: Option<(i64, i64, u64)>,
    pub cleanup_stale: bool,
}

pub(crate) fn overall(input: &Inputs) -> Overall {
    let (total, errors, p95) = input.requests.unwrap_or((0, 0, 0));
    if !input.database_up || errors * 20 >= total.max(1) && total > 0 {
        return Overall::Unhealthy;
    }
    if input.peripheral_unhealthy
        || (total > 0 && errors * 100 >= total)
        || p95 > SLOW_P95_MS
        || input.cleanup_stale
    {
        return Overall::Degraded;
    }
    Overall::Healthy
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/health", get(health))
        .route_layer(axum::middleware::map_response(
            |mut response: axum::response::Response| async move {
                response
                    .headers_mut()
                    .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
                response
            },
        ))
}

#[utoipa::path(get, path = "/api/admin/health", tag = "admin",
    params(("period_days" = Option<i64>, Query, description = "1–30 days, defaults to 1")),
    responses((status = 200, body = HealthView), (status = 400, body = ErrorBody),
              (status = 401, body = ErrorBody), (status = 403, body = ErrorBody)))]
async fn health(
    _: RequireAdmin,
    State(state): State<AppState>,
    query: Result<Query<HealthQuery>, QueryRejection>,
) -> Result<Json<HealthView>, ApiError> {
    let period_days = query
        .ok()
        .map(|Query(query)| query.period_days.unwrap_or(DEFAULT_PERIOD_DAYS))
        .filter(|days| (1..=MAX_PERIOD_DAYS).contains(days))
        .ok_or_else(|| ApiError::bad_request("invalid_health_period", "期間須介於 1 至 30 天"))?;

    let started = Instant::now();
    let database_up = matches!(crate::probe_database(&state.pool).await, Ok(Ok(_)));
    let database = DatabaseHealth {
        status: if database_up { "healthy" } else { "unhealthy" },
        response_time_ms: database_up
            .then(|| u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)),
    };

    let (stats, maintenance) = tokio::join!(
        async {
            if !database_up {
                return Err(());
            }
            match tokio::time::timeout(
                STATS_TIMEOUT,
                request_stats::summarize(&state.pool, period_days),
            )
            .await
            {
                Ok(Ok(summary)) => Ok(summary),
                Ok(Err(error)) => {
                    tracing::warn!(class = %crate::error::db_error_class(&error), "request stats read failed");
                    Err(())
                }
                Err(_) => {
                    tracing::warn!("request stats read timed out");
                    Err(())
                }
            }
        },
        async { database_up && state.maintenance.is_on(&state.pool).await },
    );
    let stats_available = stats.is_ok();
    let summary = stats.ok().flatten();

    let services = state.service_monitor.summary();
    let now = Utc::now();
    let started_at = state.process_started_at;
    let last_cleanup = match state.last_cleanup.load(Ordering::Relaxed) {
        0 => None,
        seconds => DateTime::from_timestamp(seconds, 0),
    };
    // 剛啟動還沒跑過清理時，以程序啟動時間起算
    let cleanup_stale = now - last_cleanup.unwrap_or(started_at) > CLEANUP_STALE_AFTER;

    let status = overall(&Inputs {
        database_up,
        peripheral_unhealthy: services.unhealthy > 0,
        requests: summary
            .as_ref()
            .map(|s| (s.total, s.server_errors, s.p95_ms)),
        cleanup_stale,
    });

    Ok(Json(HealthView {
        status,
        period_days,
        database,
        services,
        pool: PoolHealth {
            size: state.pool.size(),
            idle: u32::try_from(state.pool.num_idle()).unwrap_or(u32::MAX),
            max: state.pool.options().get_max_connections(),
        },
        process: ProcessHealth {
            started_at,
            uptime_seconds: (now - started_at).num_seconds().max(0),
            version: state.version.clone(),
        },
        maintenance,
        cleanup: CleanupHealth {
            last_success_at: last_cleanup,
        },
        stats_available,
        requests: summary.map(|s| RequestHealth {
            total: s.total,
            error_5xx_ratio: s.server_errors as f64 / s.total as f64,
            p95_ms: s.p95_ms,
        }),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> Inputs {
        Inputs {
            database_up: true,
            peripheral_unhealthy: false,
            requests: Some((1000, 0, 100)),
            cleanup_stale: false,
        }
    }

    #[test]
    fn thresholds() {
        assert_eq!(overall(&inputs()), Overall::Healthy);
        let with = |f: &dyn Fn(&mut Inputs)| {
            let mut i = inputs();
            f(&mut i);
            overall(&i)
        };
        assert_eq!(with(&|i| i.database_up = false), Overall::Unhealthy);
        assert_eq!(
            with(&|i| i.requests = Some((100, 5, 100))),
            Overall::Unhealthy
        );
        assert_eq!(
            with(&|i| i.requests = Some((100, 4, 100))),
            Overall::Degraded
        );
        assert_eq!(
            with(&|i| i.requests = Some((100, 1, 100))),
            Overall::Degraded
        );
        assert_eq!(
            with(&|i| i.requests = Some((1000, 9, 100))),
            Overall::Healthy
        );
        assert_eq!(
            with(&|i| i.requests = Some((100, 0, 3000))),
            Overall::Healthy
        );
        assert_eq!(
            with(&|i| i.requests = Some((100, 0, 5000))),
            Overall::Degraded
        );
        assert_eq!(with(&|i| i.peripheral_unhealthy = true), Overall::Degraded);
        assert_eq!(with(&|i| i.cleanup_stale = true), Overall::Degraded);
        assert_eq!(with(&|i| i.requests = None), Overall::Healthy);
    }
}
