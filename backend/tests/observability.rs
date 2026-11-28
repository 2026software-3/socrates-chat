//! Tracing 與 Metrics 的端對端整合測試（不需資料庫，使用 lazy pool）。
//! 涵蓋 W3C `traceparent` 傳遞、`GET /api/admin/metrics` 的驗證、Prometheus 匯出格式與零個資驗證。
//! （結構化日誌擷取測試在獨立檔案 `observability_logs.rs`，避免多執行緒共用全域 tracing 訂閱者。）

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use common::*;
use socrates_chat_backend::{config::Config, metrics::PROMETHEUS_CONTENT_TYPE, openapi, telemetry};
use sqlx::postgres::PgPoolOptions;

const METRICS_SECRET: &str = "synthetic-metrics-bearer-token-9876543210";
const UNTRUSTED_ORIGIN: &str = "https://evil-secret-origin-12345.example";

fn lazy_app(metrics_token: Option<&str>) -> axum::Router {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://unused@127.0.0.1:1/unused")
        .unwrap();
    let config = Config {
        metrics_token: metrics_token.map(str::to_owned),
        ..test_config()
    };
    test_app_with_config(pool, FakeAi::with(vec![]), config)
}

#[tokio::test]
async fn traceparent_is_generated_propagated_and_validated_on_responses() {
    let app = lazy_app(None);

    // 1. 沒有帶 traceparent 時會自動產生合法的 W3C traceparent 與 x-request-id
    let res = send(&app, Request::get("/health").body(Body::empty()).unwrap()).await;
    assert_eq!(res.status(), StatusCode::OK);
    let rid = res.headers()[telemetry::REQUEST_ID_HEADER]
        .to_str()
        .unwrap();
    assert!(uuid::Uuid::parse_str(rid).is_ok());
    let tp = res.headers()[telemetry::TRACEPARENT_HEADER]
        .to_str()
        .unwrap();
    let parts: Vec<&str> = tp.split('-').collect();
    assert_eq!(parts.len(), 4);
    assert_eq!(parts[0], "00");
    assert_eq!(parts[1].len(), 32);
    assert_eq!(parts[2].len(), 16);
    assert_eq!(parts[3], "01");

    // 2. 帶入合法的上游 traceparent 時保留 trace_id 與 flags，並產生新的子 span_id
    let upstream_trace = "4bf92f3577b34da6a3ce929d0e0e4736";
    let upstream_parent = "00f067aa0ba902b7";
    let incoming = format!("00-{upstream_trace}-{upstream_parent}-00");
    let res = send(
        &app,
        Request::get("/health")
            .header(telemetry::TRACEPARENT_HEADER, &incoming)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let echoed = res.headers()[telemetry::TRACEPARENT_HEADER]
        .to_str()
        .unwrap();
    let parts: Vec<&str> = echoed.split('-').collect();
    assert_eq!(parts[0], "00");
    assert_eq!(parts[1], upstream_trace);
    assert_ne!(parts[2], upstream_parent);
    assert_eq!(parts[3], "00");

    // 3. 帶入格式無效或全零的 traceparent 時必須安全忽略，產生全新的合法 trace_id
    for bad in [
        "invalid",
        "00-00000000000000000000000000000000-00f067aa0ba902b7-01",
        "ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        "00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01",
    ] {
        let res = send(
            &app,
            Request::get("/health")
                .header(telemetry::TRACEPARENT_HEADER, bad)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let generated = res.headers()[telemetry::TRACEPARENT_HEADER]
            .to_str()
            .unwrap();
        assert_ne!(generated, bad);
        assert!(generated.starts_with("00-"));
    }
}

#[tokio::test]
async fn metrics_endpoint_enforces_auth_and_exposes_prometheus_metrics() {
    // 1. METRICS_TOKEN 未設定時，任何 Bearer token 都必須回 401
    let no_token_app = lazy_app(None);
    let res = send(
        &no_token_app,
        Request::get("/api/admin/metrics")
            .header(header::AUTHORIZATION, format!("Bearer {METRICS_SECRET}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_json_error(res, StatusCode::UNAUTHORIZED, "unauthorized").await;

    let app = lazy_app(Some(METRICS_SECRET));

    // 2. 未帶憑證或錯誤 Bearer token 都回 401
    let unauth = send(&app, get("/api/admin/metrics", None)).await;
    assert_json_error(unauth, StatusCode::UNAUTHORIZED, "unauthorized").await;

    let wrong = send(
        &app,
        Request::get("/api/admin/metrics")
            .header(header::AUTHORIZATION, "Bearer wrong-secret")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_json_error(wrong, StatusCode::UNAUTHORIZED, "unauthorized").await;

    // 3. 製造幾筆不同結果的請求
    for _ in 0..3 {
        assert_eq!(
            send(&app, get("/health", None)).await.status(),
            StatusCode::OK
        );
    }
    assert_eq!(
        send(&app, get("/api/secret-path/bob@example.com", None))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            &app,
            Request::post("/api/conversations")
                .header(header::ORIGIN, UNTRUSTED_ORIGIN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );

    // 4. 正確 Bearer token（不區分 scheme 大小寫）可取得 Prometheus 文字格式指標
    let lowercase_bearer = send(
        &app,
        Request::get("/api/admin/metrics")
            .header(header::AUTHORIZATION, format!("bearer {METRICS_SECRET}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(lowercase_bearer.status(), StatusCode::OK);

    let res = send(
        &app,
        Request::get("/api/admin/metrics")
            .header(header::AUTHORIZATION, format!("Bearer {METRICS_SECRET}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CONTENT_TYPE], PROMETHEUS_CONTENT_TYPE);
    assert_eq!(res.headers()[header::CACHE_CONTROL], "no-store");

    let body = text_body(res).await;
    // 絕不洩漏敏感資訊或原始 URL
    assert!(!body.contains(METRICS_SECRET));
    assert!(!body.contains("bob@example.com"));
    assert!(!body.contains(UNTRUSTED_ORIGIN));

    // Counter、Gauge、Histogram 三大指標類型都正確輸出
    assert!(body.contains("# TYPE socrates_http_requests_total counter"));
    assert!(body.contains("# TYPE socrates_http_requests_aborted_total counter"));
    assert!(body.contains("# TYPE socrates_translations_total counter"));
    assert!(body.contains("# TYPE socrates_translation_duration_seconds histogram"));
    assert!(body.contains(
        "socrates_http_requests_total{method=\"GET\",route=\"/health\",status_class=\"2xx\"} 3"
    ));
    assert!(body.contains(
        "socrates_http_requests_total{method=\"GET\",route=\"<fallback>\",status_class=\"4xx\"} 1"
    ));
    assert!(body.contains(
        "socrates_http_request_errors_total{route=\"<fallback>\",error_code=\"not_found\"} 1"
    ));
    assert!(body.contains("error_code=\"csrf\"} 1"));

    assert!(body.contains("# TYPE socrates_http_request_duration_seconds histogram"));
    assert!(body.contains(
        "socrates_http_request_duration_seconds_bucket{method=\"GET\",route=\"/health\",le=\"+Inf\"} 3"
    ));
    assert!(body.contains(
        "socrates_http_request_duration_seconds_count{method=\"GET\",route=\"/health\"} 3"
    ));
    assert!(
        body.contains(
            "socrates_http_request_duration_seconds_sum{method=\"GET\",route=\"/health\"} "
        )
    );

    assert!(body.contains("# TYPE socrates_http_requests_in_flight gauge"));
    // 正在處理這次 scrape 請求本身，所以 in-flight 為 1
    assert!(body.contains("socrates_http_requests_in_flight 1"));
    assert!(body.contains("# TYPE socrates_db_pool_connections gauge"));
    assert!(body.contains("socrates_db_pool_connections{state=\"max\"} "));
    assert!(body.contains("# TYPE socrates_service_up gauge"));
    assert!(body.contains("socrates_service_up{service=\"database\"} "));
    assert!(body.contains("# TYPE socrates_process_uptime_seconds gauge"));
    assert!(body.contains("# TYPE socrates_build_info gauge"));
    assert!(body.contains("# TYPE socrates_live_replies_active gauge"));
    assert!(body.contains("# TYPE socrates_voice_sessions_active gauge"));
    assert!(body.contains("# TYPE socrates_maintenance_mode gauge"));
}

#[test]
fn metrics_endpoint_is_documented_in_openapi() {
    let doc = serde_json::to_value(openapi::document()).unwrap();
    let op = &doc["paths"]["/api/admin/metrics"]["get"];
    assert!(op.is_object(), "missing GET /api/admin/metrics in OpenAPI");
    assert!(op["responses"]["200"].is_object());
    assert!(op["responses"]["401"].is_object());
    assert!(op["responses"]["403"].is_object());
}
