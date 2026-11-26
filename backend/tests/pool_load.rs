//! 連線池上限的壓力測量（S-08.3）。**預設不跑**（`#[ignore]`）：它花數十秒、結果隨機器與資料庫設定而變，
//! 不適合當每次 CI 的通過條件。用來回答「連線池上限設多少，服務在多大的併發下仍然正常」。
//!
//! 執行（需要 `DATABASE_URL`，合成資料）：
//!
//! ```text
//! cargo test --test pool_load -- --ignored --nocapture
//! ```
//!
//! 可用環境變數調整：`LOAD_POOL_SIZES`（預設 `2,5,10,20,40`）、`LOAD_CONCURRENCY`（預設 `25,100,400,800,1600`）、
//! `LOAD_REQUESTS`（每一格固定發出的請求總數，預設 20000；不是跑固定秒數，所以不需要睡眠計時）。
//!
//! 負載：同時有 N 位學生不間斷（沒有思考時間，是最壞情況）共同領取並發出固定數量的請求，輪流呼叫 `/api/me`、`/api/conversations`、`/api/points`，
//! 每個請求都要驗 session 並至少查詢一次資料庫。
//!
//! 「服務正常」的判準（與 S-09.3 的服務健康度一致的精神）：沒有任何非 2xx 回應或逾時，且 p95 延遲不超過
//! [`P95_LIMIT`]。每個連線池上限列出「仍然正常的最高併發」，輸出表格最後一行是建議。

mod common;

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{Router, http::StatusCode};
use common::*;
use sqlx::{PgPool, postgres::PgConnectOptions, postgres::PgPoolOptions};
use tower::ServiceExt;

/// 「正常」的延遲上限：p95 超過就算降級。
const P95_LIMIT: Duration = Duration::from_millis(1000);

const PATHS: [&str; 3] = ["/api/me", "/api/conversations", "/api/points"];

fn env_list(name: &str, default: &[usize]) -> Vec<usize> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| {
            v.split(',')
                .map(|p| p.trim().parse().expect("invalid list value"))
                .collect()
        })
        .unwrap_or_else(|| default.to_vec())
}

struct Outcome {
    requests: usize,
    errors: usize,
    p50: Duration,
    p95: Duration,
    rps: f64,
}

impl Outcome {
    fn healthy(&self) -> bool {
        self.errors == 0 && self.requests > 0 && self.p95 <= P95_LIMIT
    }
}

/// N 位併發使用者共同發出 `total` 個請求：每人用共用計數器領下一個編號，領完就停。
async fn drive(app: &Router, cookie: &str, users: usize, total: usize) -> Outcome {
    let next = Arc::new(AtomicUsize::new(0));
    let errors = Arc::new(AtomicUsize::new(0));
    let started = Instant::now();
    let mut tasks = Vec::new();
    for _ in 0..users {
        let (app, cookie, next, errors) = (
            app.clone(),
            cookie.to_string(),
            next.clone(),
            errors.clone(),
        );
        tasks.push(tokio::spawn(async move {
            let mut latencies = Vec::new();
            loop {
                let n = next.fetch_add(1, Ordering::Relaxed);
                if n >= total {
                    break;
                }
                let at = Instant::now();
                let ok = matches!(
                    app.clone()
                        .oneshot(get(PATHS[n % PATHS.len()], Some(&cookie)))
                        .await,
                    Ok(res) if res.status() == StatusCode::OK
                );
                // flaky-ok: 壓測測量工具（#[ignore]，不進 CI）；時間只用來印出表格，沒有任何通過／失敗的斷言，機器變慢只會讓數字變差
                latencies.push(at.elapsed());
                if !ok {
                    errors.fetch_add(1, Ordering::Relaxed);
                }
            }
            latencies
        }));
    }
    let mut all: Vec<Duration> = Vec::new();
    for task in tasks {
        all.extend(task.await.unwrap());
    }
    // flaky-ok: 壓測測量工具（#[ignore]，不進 CI）；時間只用來印出表格，沒有任何通過／失敗的斷言，機器變慢只會讓數字變差
    let elapsed = started.elapsed().as_secs_f64();
    all.sort();
    let at = |q: f64| {
        all.get(((all.len() as f64) * q) as usize)
            .or(all.last())
            .copied()
            .unwrap_or_default()
    };
    Outcome {
        requests: all.len(),
        errors: errors.load(Ordering::Relaxed),
        p50: at(0.50),
        p95: at(0.95),
        rps: all.len() as f64 / elapsed,
    }
}

async fn measure(options: PgConnectOptions) {
    let total: usize = std::env::var("LOAD_REQUESTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20_000);
    let sizes = env_list("LOAD_POOL_SIZES", &[2, 5, 10, 20, 40]);
    let levels = env_list("LOAD_CONCURRENCY", &[25, 100, 400, 800, 1600]);

    // 資料只建一次（教室、名單、學生），之後每個連線池大小共用同一個 session cookie：
    // 名單內的學生才是正常請求，名單外的帳號會被擋下，量到的就不是正常的負載。
    let setup = PgPoolOptions::new()
        .max_connections(5)
        .connect_with(options.clone())
        .await
        .unwrap();
    let cookie = seed_actors(&test_app(setup)).await.student;

    println!("pool  users  requests  errors  p50(ms)  p95(ms)   req/s  status");
    let mut best: Vec<(usize, usize)> = Vec::new();
    for &size in &sizes {
        let pool: PgPool = PgPoolOptions::new()
            .max_connections(size as u32)
            .connect_with(options.clone())
            .await
            .unwrap();
        let app = test_app(pool);
        for path in PATHS {
            let res = app.clone().oneshot(get(path, Some(&cookie))).await.unwrap();
            assert_eq!(
                res.status(),
                StatusCode::OK,
                "{path} should work before load"
            );
        }
        let mut highest = 0;
        for &users in &levels {
            let o = drive(&app, &cookie, users, total).await;
            println!(
                "{size:>4}  {users:>5}  {:>8}  {:>6}  {:>7.1}  {:>7.1}  {:>6.0}  {}",
                o.requests,
                o.errors,
                o.p50.as_secs_f64() * 1000.0,
                o.p95.as_secs_f64() * 1000.0,
                o.rps,
                if o.healthy() { "ok" } else { "DEGRADED" }
            );
            if o.healthy() {
                highest = users;
            } else {
                break;
            }
        }
        best.push((size, highest));
    }
    println!("\n連線池上限 → 仍然正常的最高併發使用者數（p95 ≤ 1s 且零錯誤）：");
    for (size, highest) in best {
        println!("  {size:>3} → {highest}");
    }
}

#[sqlx::test]
#[ignore = "壓力測量，手動執行：cargo test --test pool_load -- --ignored --nocapture"]
async fn find_highest_sustainable_load_per_pool_size(pool: PgPool) {
    let options: PgConnectOptions = (*pool.connect_options()).clone();
    // 在自己的多執行緒 runtime 執行：sqlx::test 預設的單執行緒 runtime 會讓 CPU 先成為瓶頸，量不到連線池。
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(measure(options));
    })
    .join()
    .unwrap();
}
