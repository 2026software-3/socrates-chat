mod common;

use std::sync::Arc;

use axum::http::StatusCode;
use common::*;
use serde_json::json;
use socrates_chat_backend::{AppState, app, diagnostics, service_monitor::MonitoredService};
use sqlx::PgPool;

const HEALTH: &str = "/api/admin/health";

fn state_with(pool: PgPool, config: socrates_chat_backend::config::Config) -> AppState {
    register_fixture_pool(&pool);
    AppState::new(pool, config, Arc::new(FakeIdentity), FakeAi::with(vec![]))
}

async fn insert_hour(pool: &PgPool, hours_ago: i32, total: i64, s5xx: i64, bucket: &str) {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO request_stats_hourly (hour, total, status_5xx, {bucket})
         VALUES (date_trunc('hour', now()) - make_interval(hours => $1), $2, $3, $2)"
    )))
    .bind(hours_ago)
    .bind(total)
    .bind(s5xx)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test]
async fn only_admins_can_read_health(pool: PgPool) {
    let app = test_app(pool);
    let actors = seed_actors(&app).await;
    for (cookie, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(actors.teacher.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.student.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.outsider.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.admin.as_str()), StatusCode::OK),
    ] {
        assert_eq!(send(&app, get(HEALTH, cookie)).await.status(), expected);
    }
}

#[sqlx::test]
async fn reports_the_full_overview(pool: PgPool) {
    let app = test_app(pool.clone());
    let actors = seed_actors(&app).await;
    let body = json_body(send(&app, get(HEALTH, Some(&actors.admin))).await).await;
    assert_eq!(body["status"], "healthy");
    assert_eq!(body["database"]["status"], "healthy");
    assert!(body["database"]["response_time_ms"].is_number());
    assert_eq!(body["services"]["total"], 0);
    assert!(body["pool"]["max"].as_u64().unwrap() >= 1);
    assert!(body["process"]["uptime_seconds"].is_number());
    assert!(body["process"]["version"].is_string());
    assert_eq!(body["maintenance"], false);
    assert_eq!(body["stats_available"], true);
    assert_eq!(body["period_days"], 1);
    // 期間內沒有請求（種子資料的請求還沒有寫入）
    assert!(body["requests"].is_null());
    for period in [0, 31] {
        let url = format!("{HEALTH}?period_days={period}");
        assert_eq!(
            send(&app, get(&url, Some(&actors.admin))).await.status(),
            StatusCode::BAD_REQUEST
        );
    }
}

#[sqlx::test]
async fn request_statistics_are_flushed_as_totals_and_summed_across_processes(pool: PgPool) {
    let state_a = state_with(pool.clone(), test_config());
    let state_b = state_with(pool.clone(), test_config());
    let app_a = app(state_a.clone());
    let app_b = app(state_b.clone());
    for _ in 0..3 {
        send(&app_a, get("/health", None)).await;
    }
    send(&app_a, get("/api/nope", None)).await; // 404
    for _ in 0..2 {
        send(&app_b, get("/health", None)).await;
    }
    tokio::join!(state_a.flush_request_stats(), state_b.flush_request_stats());
    let (rows, total, ok, not_found): (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT count(*), sum(total)::bigint, sum(status_2xx)::bigint, sum(status_4xx)::bigint
         FROM request_stats_hourly",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((rows, total, ok, not_found), (1, 6, 5, 1));
    // 之後再來的請求合併進同一列
    send(&app_a, get("/health", None)).await;
    state_a.flush_request_stats().await;
    let total: i64 = sqlx::query_scalar("SELECT sum(total)::bigint FROM request_stats_hourly")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(total, 7);
    // 只有全站合計：資料表沒有路由、使用者或 URL 欄位
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name FROM information_schema.columns WHERE table_name = 'request_stats_hourly'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(
        columns
            .iter()
            .all(|c| !["route", "user_id", "url", "ip", "path"].contains(&c.as_str()))
    );
}

#[sqlx::test]
async fn failing_to_store_statistics_does_not_fail_requests(pool: PgPool) {
    let state = state_with(pool.clone(), test_config());
    let app = app(state.clone());
    sqlx::query("DROP TABLE request_stats_hourly")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        send(&app, get("/health", None)).await.status(),
        StatusCode::OK
    );
    state.flush_request_stats().await;
    assert_eq!(
        send(&app, get("/health", None)).await.status(),
        StatusCode::OK
    );
}

#[sqlx::test]
async fn request_totals_error_ratio_and_p95_follow_the_period(pool: PgPool) {
    let app = test_app(pool.clone());
    let actors = seed_actors(&app).await;
    // 24 小時內：100 筆、1 筆 5xx、全部在 ≤100ms 的分桶
    insert_hour(&pool, 2, 100, 1, "lat_100ms").await;
    // 3 天前：900 筆，只在 7 天與 30 天的期間內
    insert_hour(&pool, 72, 900, 0, "lat_100ms").await;
    // 31 天前：不得出現
    insert_hour(&pool, 24 * 31, 5000, 5000, "lat_100ms").await;
    let read = |days: u32| {
        let app = app.clone();
        let cookie = actors.admin.clone();
        async move {
            json_body(
                send(
                    &app,
                    get(&format!("{HEALTH}?period_days={days}"), Some(&cookie)),
                )
                .await,
            )
            .await
        }
    };
    let day = read(1).await;
    assert_eq!(day["requests"]["total"], 100);
    assert_eq!(day["requests"]["error_5xx_ratio"], 0.01);
    assert_eq!(day["requests"]["p95_ms"], 100);
    // 5xx 比例剛好 1% → 降級
    assert_eq!(day["status"], "degraded");
    let week = read(7).await;
    assert_eq!(week["requests"]["total"], 1000);
    assert_eq!(week["status"], "healthy");
    let month = read(30).await;
    assert_eq!(month["requests"]["total"], 1000);
}

#[sqlx::test]
async fn high_error_ratio_is_unhealthy_and_slow_p95_is_degraded(pool: PgPool) {
    let app = test_app(pool.clone());
    let actors = seed_actors(&app).await;
    insert_hour(&pool, 1, 100, 5, "lat_100ms").await;
    let body = json_body(send(&app, get(HEALTH, Some(&actors.admin))).await).await;
    assert_eq!(body["status"], "unhealthy");

    sqlx::query("DELETE FROM request_stats_hourly")
        .execute(&pool)
        .await
        .unwrap();
    insert_hour(&pool, 1, 100, 0, "lat_5000ms").await;
    let body = json_body(send(&app, get(HEALTH, Some(&actors.admin))).await).await;
    assert_eq!(body["requests"]["p95_ms"], 5000);
    assert_eq!(body["status"], "degraded");
}

#[sqlx::test]
async fn an_unhealthy_peripheral_service_degrades_the_site(pool: PgPool) {
    let mut config = test_config();
    config.monitored_services = MonitoredService::parse(
        &json!([{
            "name": "tts", "label": "TTS", "impact": "Voice",
            "url": "http://127.0.0.1:1/health"
        }])
        .to_string(),
    )
    .unwrap();
    let app = test_app_with_config(pool, FakeAi::with(vec![]), config);
    let actors = seed_actors(&app).await;
    let check = || {
        json_req(
            "POST",
            "/api/admin/services/check",
            Some(&actors.admin),
            Some(json!({"name": "tts"})),
        )
    };
    send(&app, check()).await;
    let body = json_body(send(&app, get(HEALTH, Some(&actors.admin))).await).await;
    assert_eq!(body["services"]["total"], 1);
    assert_eq!(body["services"]["healthy"], 0);
    // 單次失敗還不算異常（連續 2 次才算）
    assert_eq!(body["status"], "healthy");
}

#[sqlx::test]
async fn database_outage_still_returns_service_up_and_database_down(pool: PgPool) {
    let app = test_app(pool.clone());
    let actors = seed_actors(&app).await;
    // 先確認登入有效，再讓統計資料表消失：統計讀取失敗不整個失敗
    sqlx::query("DROP TABLE request_stats_hourly")
        .execute(&pool)
        .await
        .unwrap();
    let response = send(&app, get(HEALTH, Some(&actors.admin))).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["stats_available"], false);
    assert!(body["requests"].is_null());
    assert_eq!(body["database"]["status"], "healthy");
}

#[sqlx::test]
async fn purge_removes_request_statistics_older_than_30_days(pool: PgPool) {
    insert_hour(&pool, 24 * 31, 10, 0, "lat_100ms").await;
    insert_hour(&pool, 1, 10, 0, "lat_100ms").await;
    diagnostics::purge_expired(&pool).await.unwrap();
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM request_stats_hourly")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 1);
}
