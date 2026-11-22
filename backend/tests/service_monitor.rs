mod common;

use axum::{Router, http::StatusCode, routing::get as route_get};
use common::*;
use serde_json::json;
use socrates_chat_backend::{diagnostics, service_monitor::MonitoredService};
use sqlx::PgPool;

const LIST: &str = "/api/admin/services";
const CHECK: &str = "/api/admin/services/check";

#[sqlx::test]
async fn only_admins_can_list_or_check_and_default_is_database(pool: PgPool) {
    let app = test_app(pool);
    let actors = seed_actors(&app).await;
    for (cookie, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(actors.teacher.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.student.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.outsider.as_str()), StatusCode::FORBIDDEN),
    ] {
        assert_eq!(send(&app, get(LIST, cookie)).await.status(), expected);
        assert_eq!(
            send(
                &app,
                json_req("POST", CHECK, cookie, Some(json!({"name": "database"})))
            )
            .await
            .status(),
            expected
        );
    }
    let response = send(&app, get(LIST, Some(&actors.admin))).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["services"].as_array().unwrap().len(), 1);
    assert_eq!(body["services"][0]["name"], "database");
    assert_eq!(body["history_available"], true);
}

#[sqlx::test]
async fn manual_check_hides_credentials_and_rate_limits_and_history_can_fail(pool: PgPool) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let fake = Router::new().route(
            "/health",
            route_get(|headers: axum::http::HeaderMap| async move {
                assert!(headers.get("authorization").is_none());
                assert!(headers.get("cookie").is_none());
                StatusCode::NO_CONTENT
            }),
        );
        axum::serve(listener, fake).await.unwrap();
    });
    let mut config = test_config();
    config.monitored_services = MonitoredService::parse(
        &json!([{
            "name": "tts", "label": "TTS", "impact": "Voice",
            "url": format!("http://probe-user:probe-secret@{addr}/health?token=probe-token")
        }])
        .to_string(),
    )
    .unwrap();
    let app = test_app_with_config(pool.clone(), FakeAi::with(vec![]), config);
    let actors = seed_actors(&app).await;
    let check = || {
        json_req(
            "POST",
            CHECK,
            Some(&actors.admin),
            Some(json!({"name": "tts"})),
        )
    };
    let response = send(&app, check()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let row = json_body(response).await;
    assert_eq!(row["status"], "healthy");
    assert_eq!(row["target"], format!("{addr}/health"));
    assert!(row["response_time_ms"].is_number());
    assert_eq!(
        send(&app, check()).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );

    sqlx::query("INSERT INTO service_status_events (service_name, status, occurred_at, failure) VALUES ('tts', 'unhealthy', now() - interval '31 days', 'timeout')")
        .execute(&pool).await.unwrap();
    let body = json_body(send(&app, get(LIST, Some(&actors.admin))).await).await;
    let tts = body["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "tts")
        .unwrap();
    assert_eq!(tts["recent_events"].as_array().unwrap().len(), 1);
    assert!(tts["availability"].is_number());
    diagnostics::purge_expired(&pool).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM service_status_events")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );

    sqlx::query("DROP TABLE service_status_events")
        .execute(&pool)
        .await
        .unwrap();
    let body = json_body(send(&app, get(LIST, Some(&actors.admin))).await).await;
    assert_eq!(body["history_available"], false);
    assert!(
        body["services"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["name"] == "tts" && row["status"] == "healthy")
    );
    server.abort();
}

#[test]
fn config_errors_point_to_the_item_without_echoing_url_credentials() {
    let duplicated = json!([
        {"name": "asr", "label": "ASR", "impact": "Voice", "url": "http://asr/health"},
        {"name": "asr", "label": "ASR", "impact": "Voice", "url": "http://user:secret@asr/health"}
    ]);
    let malformed = json!([
        {"name": "asr", "label": "ASR", "impact": "Voice", "url": "ftp://user:secret@asr/health"}
    ]);
    for (input, index) in [(duplicated, 1), (malformed, 0)] {
        let error = MonitoredService::parse(&input.to_string())
            .unwrap_err()
            .to_string();
        assert!(error.contains(&format!("MONITORED_SERVICES[{index}]")));
        assert!(!error.contains("secret"));
    }
}
