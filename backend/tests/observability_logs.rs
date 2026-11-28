//! Tracing 與 Logging 整合測試（不需資料庫，使用 lazy pool）。
//! 這個檔案只能有一個測試：tracing 的全域訂閱者整個程序只能設定一次（做法同 `log_hygiene.rs`）。

mod common;

use std::{
    io,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use common::*;
use serde_json::Value;
use socrates_chat_backend::{config::Config, logging, telemetry};
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::fmt::MakeWriter;

const METRICS_SECRET: &str = "synthetic-metrics-bearer-token-9876543210";
const UNTRUSTED_ORIGIN: &str = "https://evil-secret-origin-12345.example";

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

struct Sink(Arc<Mutex<Vec<u8>>>);

impl io::Write for Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Sink;
    fn make_writer(&'a self) -> Sink {
        Sink(self.0.clone())
    }
}

#[tokio::test]
async fn structured_logs_record_trace_tree_levels_and_never_leak_secrets() {
    let logs = Captured::default();
    logging::json_subscriber()
        .with_writer(logs.clone())
        .with_env_filter(logging::env_filter(Some("info")))
        .init();

    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://unused@127.0.0.1:1/unused")
        .unwrap();
    let config = Config {
        metrics_token: Some(METRICS_SECRET.into()),
        request_timeout: Duration::from_secs(10),
        ..test_config()
    };
    let app = test_app_with_config(pool, FakeAi::with(vec![]), config);

    let upstream_trace = "abcdef0123456789abcdef0123456789";
    let upstream_parent = "1122334455667788";

    // 1. 200 OK with incoming W3C traceparent
    let ok_res = send(
        &app,
        Request::get("/health")
            .header(
                telemetry::TRACEPARENT_HEADER,
                format!("00-{upstream_trace}-{upstream_parent}-01"),
            )
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(ok_res.status(), StatusCode::OK);

    // 2. 404 Not Found with sensitive-looking raw URL path
    let missing_res = send(
        &app,
        Request::get("/api/nonexistent/alice@example.com")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(missing_res.status(), StatusCode::NOT_FOUND);

    // 3. 403 CSRF with untrusted Origin
    let csrf_res = send(
        &app,
        Request::post("/api/conversations")
            .header(header::ORIGIN, UNTRUSTED_ORIGIN)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(csrf_res.status(), StatusCode::FORBIDDEN);
    assert!(
        csrf_res
            .headers()
            .contains_key(telemetry::TRACEPARENT_HEADER)
    );

    // 4. Metrics scrape with valid bearer token
    let metrics_res = send(
        &app,
        Request::get("/api/admin/metrics")
            .header(header::AUTHORIZATION, format!("Bearer {METRICS_SECRET}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(metrics_res.status(), StatusCode::OK);

    // 5. 503 readiness check failure -> ERROR level request completed + component span
    let ready_res = send(
        &app,
        Request::get("/ready")
            .header(
                telemetry::TRACEPARENT_HEADER,
                format!("00-{upstream_trace}-{upstream_parent}-01"),
            )
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(ready_res.status(), StatusCode::SERVICE_UNAVAILABLE);

    let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(!output.contains("alice@example.com"));
    assert!(!output.contains(UNTRUSTED_ORIGIN));
    assert!(!output.contains(METRICS_SECRET));

    let entries: Vec<Value> = output
        .lines()
        .map(|l| serde_json::from_str(l).expect("every line must be valid JSON"))
        .collect();

    let health_log = entries
        .iter()
        .find(|e| {
            e["fields"]["message"] == "request completed" && e["fields"]["route"] == "/health"
        })
        .expect("health request log");
    assert_eq!(health_log["level"], "INFO");
    assert_eq!(health_log["fields"]["trace_id"], upstream_trace);
    assert_eq!(health_log["span"]["trace_id"], upstream_trace);
    assert_eq!(health_log["span"]["parent_span_id"], upstream_parent);
    assert!(health_log["fields"]["span_id"].is_string());

    let missing_log = entries
        .iter()
        .find(|e| e["fields"]["message"] == "request completed" && e["fields"]["status"] == 404)
        .expect("404 request log");
    assert_eq!(missing_log["level"], "WARN");
    assert_eq!(missing_log["fields"]["route"], "<fallback>");
    assert_eq!(missing_log["fields"]["error_code"], "not_found");

    let csrf_warn = entries
        .iter()
        .find(|e| e["fields"]["message"] == "csrf origin rejected")
        .expect("csrf warning log");
    assert_eq!(csrf_warn["level"], "WARN");

    let ready_log = entries
        .iter()
        .find(|e| e["fields"]["message"] == "request completed" && e["fields"]["route"] == "/ready")
        .expect("ready 503 request log");
    assert_eq!(ready_log["level"], "ERROR");
    assert_eq!(ready_log["fields"]["status"], 503);
    assert_eq!(ready_log["fields"]["error_code"], "database_unavailable");
    assert_eq!(ready_log["fields"]["trace_id"], upstream_trace);

    // 6. Panic hook：重複安裝具冪等性，只記錄原始碼位置與執行緒，絕不洩漏 panic payload（S-02.2）
    const PANIC_SECRET: &str = "TOP-SECRET-PANIC-PAYLOAD-987654321";
    logging::install_panic_hook();
    logging::install_panic_hook();
    let panicked = std::panic::catch_unwind(|| {
        panic!("{PANIC_SECRET}");
    });
    assert!(panicked.is_err());

    let output_after_panic = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(!output_after_panic.contains(PANIC_SECRET));
    let panic_entries: Vec<Value> = output_after_panic
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).expect("valid JSON"))
        .filter(|e| e["fields"]["message"] == "panic occurred")
        .collect();
    assert_eq!(
        panic_entries.len(),
        1,
        "install_panic_hook must be idempotent"
    );
    assert_eq!(panic_entries[0]["level"], "ERROR");
    assert!(panic_entries[0]["fields"]["file"].is_string());
    assert!(panic_entries[0]["fields"]["line"].is_u64());
    assert!(panic_entries[0]["fields"]["column"].is_u64());
    assert!(panic_entries[0]["fields"]["thread"].is_string());
}
