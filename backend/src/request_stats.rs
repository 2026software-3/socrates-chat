//! 全站請求統計（F-39.2 / S-09.3）：請求完成時在記憶體累加合計，每分鐘合併寫入每小時一列。
//! 只有請求數、狀態碼類別數與耗時分桶；不含路由、使用者、IP 或 URL。

use std::{collections::BTreeMap, time::Duration};

use chrono::{DateTime, DurationRound, TimeDelta, Utc};
use sqlx::PgPool;

use crate::mutex::lock;

/// 耗時分桶的上限（毫秒）；最後一個分桶是超過 10 秒。
pub(crate) const BUCKET_UPPER_MS: [u64; 9] = [50, 100, 250, 500, 1000, 2000, 3000, 5000, 10000];
const BUCKETS: usize = BUCKET_UPPER_MS.len() + 1;
const BUCKET_COLUMNS: [&str; BUCKETS] = [
    "lat_50ms",
    "lat_100ms",
    "lat_250ms",
    "lat_500ms",
    "lat_1000ms",
    "lat_2000ms",
    "lat_3000ms",
    "lat_5000ms",
    "lat_10000ms",
    "lat_over",
];
const CLASS_COLUMNS: [&str; 4] = ["status_2xx", "status_3xx", "status_4xx", "status_5xx"];
pub(crate) const FLUSH_INTERVAL: Duration = Duration::from_secs(60);
const FLUSH_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Default)]
struct Counts {
    total: i64,
    classes: [i64; 4],
    buckets: [i64; BUCKETS],
}

impl Counts {
    fn add(&mut self, other: &Counts) {
        self.total += other.total;
        for (a, b) in self.classes.iter_mut().zip(other.classes) {
            *a += b;
        }
        for (a, b) in self.buckets.iter_mut().zip(other.buckets) {
            *a += b;
        }
    }
}

#[derive(Default)]
pub(crate) struct RequestStats {
    pending: std::sync::Mutex<BTreeMap<DateTime<Utc>, Counts>>,
}

fn current_hour() -> DateTime<Utc> {
    Utc::now()
        .duration_trunc(TimeDelta::hours(1))
        .unwrap_or_else(|_| Utc::now())
}

impl RequestStats {
    pub(crate) fn record(&self, status: u16, duration_ms: u64) {
        let mut counts = Counts {
            total: 1,
            ..Counts::default()
        };
        if let 200..=599 = status
            && let Some(class) = usize::from(status / 100).checked_sub(2)
        {
            counts.classes[class] = 1;
        }
        let bucket = BUCKET_UPPER_MS
            .iter()
            .position(|&upper| duration_ms <= upper)
            .unwrap_or(BUCKETS - 1);
        counts.buckets[bucket] = 1;
        lock(&self.pending)
            .entry(current_hour())
            .or_default()
            .add(&counts);
    }

    /// 把累加的合計寫入資料庫。失敗時放回去，下一輪再寫；只以 tracing 記錄。回傳這輪是否全數寫入成功。
    pub(crate) async fn flush(&self, pool: &PgPool) -> bool {
        let taken = std::mem::take(&mut *lock(&self.pending));
        let mut failed = BTreeMap::new();
        for (hour, counts) in taken {
            match tokio::time::timeout(FLUSH_TIMEOUT, write_hour(pool, hour, &counts)).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    tracing::warn!(class = %crate::error::db_error_class(&error), "request stats write failed");
                    failed.insert(hour, counts);
                }
                Err(_) => {
                    tracing::warn!("request stats write timed out");
                    failed.insert(hour, counts);
                }
            }
        }
        let all_ok = failed.is_empty();
        if !all_ok {
            let mut pending = lock(&self.pending);
            for (hour, counts) in failed {
                pending.entry(hour).or_default().add(&counts);
            }
            // 資料庫長時間不通時只保留最近的小時，避免記憶體無限成長
            while pending.len() > 48 {
                pending.pop_first();
            }
        }
        all_ok
    }
}

// SQL 只由本檔的常數欄位名稱組成，沒有任何外部輸入。
async fn write_hour(
    pool: &PgPool,
    hour: DateTime<Utc>,
    counts: &Counts,
) -> Result<(), sqlx::Error> {
    let columns: Vec<&str> = CLASS_COLUMNS
        .iter()
        .chain(&BUCKET_COLUMNS)
        .copied()
        .collect();
    let values: Vec<i64> = counts
        .classes
        .iter()
        .chain(&counts.buckets)
        .copied()
        .collect();
    let names = columns.join(", ");
    let params: String = (0..columns.len())
        .map(|i| format!(", ${}", i + 3))
        .collect();
    let updates: String = columns
        .iter()
        .map(|c| format!(", {c} = request_stats_hourly.{c} + EXCLUDED.{c}"))
        .collect();
    let sql = format!(
        "INSERT INTO request_stats_hourly (hour, total, {names}) VALUES ($1, $2{params})
         ON CONFLICT (hour) DO UPDATE SET total = request_stats_hourly.total + EXCLUDED.total{updates}"
    );
    let mut query = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(hour)
        .bind(counts.total);
    for value in values {
        query = query.bind(value);
    }
    query.execute(pool).await.map(|_| ())
}

/// 期間內的全站合計。
pub(crate) struct Summary {
    pub total: i64,
    pub server_errors: i64,
    pub p95_ms: u64,
}

pub(crate) async fn summarize(
    pool: &PgPool,
    period_days: i64,
) -> Result<Option<Summary>, sqlx::Error> {
    let sums: Vec<String> = BUCKET_COLUMNS
        .iter()
        .map(|c| format!("COALESCE(sum({c}), 0)::bigint"))
        .collect();
    let sql = format!(
        "SELECT COALESCE(sum(total), 0)::bigint, COALESCE(sum(status_5xx), 0)::bigint, {}
         FROM request_stats_hourly
         WHERE hour >= date_trunc('hour', now() - make_interval(days => $1::int))
           AND hour >= now() - interval '720 hours'",
        sums.join(", ")
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(period_days)
        .fetch_one(pool)
        .await?;
    use sqlx::Row;
    let total: i64 = row.try_get(0)?;
    if total == 0 {
        return Ok(None);
    }
    let server_errors: i64 = row.try_get(1)?;
    let target = (total * 95 + 99) / 100;
    let mut cumulative = 0;
    let mut p95_ms = *BUCKET_UPPER_MS.last().expect("buckets");
    for (index, upper) in BUCKET_UPPER_MS.iter().enumerate() {
        cumulative += row.try_get::<i64, _>(index + 2)?;
        if cumulative >= target {
            p95_ms = *upper;
            break;
        }
    }
    Ok(Some(Summary {
        total,
        server_errors,
        p95_ms,
    }))
}

pub(crate) fn spawn(state: &crate::AppState) {
    let state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(FLUSH_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            crate::telemetry::in_component_span("background_job", "request_stats_flush", async {
                let job_metric = state.metrics.track_background_job("request_stats_flush");
                let ok = state.request_stats.flush(&state.pool).await;
                job_metric.finish(if ok { "ok" } else { "failed" });
            })
            .await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_class_and_bucket() {
        let stats = RequestStats::default();
        stats.record(200, 10);
        stats.record(503, 6000);
        stats.record(404, 20000);
        let pending = lock(&stats.pending);
        let counts = pending.values().next().unwrap();
        assert_eq!(counts.total, 3);
        assert_eq!(counts.classes, [1, 0, 1, 1]);
        assert_eq!(counts.buckets[0], 1);
        assert_eq!(counts.buckets[8], 1);
        assert_eq!(counts.buckets[9], 1);
    }
}
