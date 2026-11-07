//! S-09.3 就緒檢查 `GET /ready`：以 `SELECT 1` 確認資料庫連得上，成功 200、失敗 503。
//! 與 `/health`（只表示程序存活）不同；探針不需登入，失敗時只回固定的錯誤代碼。
//! 使用 `sqlx::test` 與合成資料，需要 `DATABASE_URL`（見 docker-compose.yml）。

mod common;

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Router,
    http::{StatusCode, header},
};
use common::*;
use serde_json::{Value, json};
use socrates_chat_backend::{AppState, app, openapi};
use sqlx::{PgPool, postgres::PgPoolOptions};

/// 連不上的資料庫：連線字串刻意帶有可辨識的帳密、主機與資料庫名稱，
/// 用來確認回應與日誌不會把它們帶出去。
const SECRET_URL: &str = "postgres://leaky_user:leaky_pass@127.0.0.1:1/leaky_db";

fn unreachable_pool() -> PgPool {
    PgPoolOptions::new().connect_lazy(SECRET_URL).unwrap()
}

/// 掛上前端建置產物的 app，用來確認 `/ready` 走的是 API 路由，而不是 SPA fallback。
fn app_with_frontend(pool: PgPool) -> (Router, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("socrates-ready-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "<!doctype html><title>SPA</title>").unwrap();
    let mut config = test_config();
    config.frontend_dir = Some(dir.clone());
    let state = AppState::new(pool, config, Arc::new(FakeIdentity), FakeAi::with(vec![]));
    (app(state), dir)
}

#[sqlx::test]
async fn ready_returns_200_when_database_is_reachable(pool: PgPool) {
    let app = test_app(pool);

    // 不帶任何 cookie：探針不需登入
    let res = send(&app, get("/ready", None)).await;

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "application/json");
    assert_eq!(json_body(res).await, json!({ "status": "ok" }));
}

#[sqlx::test]
async fn ready_is_an_api_route_not_the_spa_fallback(pool: PgPool) {
    let (app, dir) = app_with_frontend(pool);

    let res = send(&app, get("/ready", None)).await;

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "application/json");
    let body = text_body(res).await;
    assert!(!body.contains("<title>SPA</title>"), "got the SPA: {body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap(),
        json!({ "status": "ok" })
    );
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn ready_returns_503_when_database_is_unreachable() {
    let app = test_app(unreachable_pool());

    let res = send(&app, get("/ready", None)).await;

    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "application/json");
    let body = json_body(res).await;
    assert_eq!(body["error"]["code"], "database_unavailable");
}

#[tokio::test]
async fn ready_returns_503_when_pool_is_closed() {
    let pool = unreachable_pool();
    pool.close().await;
    let app = test_app(pool);

    let res = send(&app, get("/ready", None)).await;

    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = json_body(res).await;
    assert_eq!(body["error"]["code"], "database_unavailable");
}

#[tokio::test]
async fn ready_failure_is_json_even_with_frontend_mounted() {
    let (app, dir) = app_with_frontend(unreachable_pool());

    let res = send(&app, get("/ready", None)).await;

    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "application/json");
    assert!(!text_body(res).await.contains("<title>SPA</title>"));
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn ready_failure_body_leaks_nothing_internal() {
    let app = test_app(unreachable_pool());

    let res = send(&app, get("/ready", None)).await;

    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = text_body(res).await;
    let parsed: Value = serde_json::from_str(&body).unwrap();
    // 與其他錯誤相同的格式：只有固定的 code 與 request_id
    let error = parsed["error"].as_object().unwrap();
    let mut keys: Vec<_> = error.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["code", "request_id"]);
    assert_eq!(parsed.as_object().unwrap().len(), 1);

    let lower = body.to_lowercase();
    for leaked in [
        "leaky_user",
        "leaky_pass",
        "leaky_db",
        "127.0.0.1",
        "postgres",
        "refused",
        "sqlx",
        "select",
        "panicked",
        "backtrace",
        "stack",
    ] {
        assert!(!lower.contains(leaked), "body leaks `{leaked}`: {body}");
    }
}

/// 資料庫卡住（連線都被占住、取不到新連線）時，探針要在合理時間內回 503，不能跟著卡住。
/// 單一連線的 pool 被測試自己占住；沒有逾時的話會等到 pool 預設的 30 秒。
#[sqlx::test]
async fn ready_returns_503_in_bounded_time_when_database_hangs(pool: PgPool) {
    let hung = PgPoolOptions::new()
        .max_connections(1)
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap();
    let _held = hung.acquire().await.unwrap();
    let app = test_app(hung.clone());

    let started = Instant::now();
    let res = send(&app, get("/ready", None)).await;

    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        json_body(res).await["error"]["code"],
        "database_unavailable"
    );
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "ready took {:?}",
        started.elapsed()
    );
}

/// 探針每隔幾秒就會呼叫，成功的就緒檢查不進管理者的 request log ring（與 `/health` 相同），
/// 以免擠掉真正的應用請求。
#[sqlx::test]
async fn successful_ready_probe_stays_out_of_the_request_log_ring(pool: PgPool) {
    let app = test_app(pool);
    let actors = seed_actors(&app).await;
    assert_eq!(
        send(&app, get("/ready", None)).await.status(),
        StatusCode::OK
    );

    let body = json_body(send(&app, get("/api/admin/logs", Some(&actors.admin))).await).await;
    let items = body["items"].as_array().unwrap();
    assert!(
        items.iter().all(|entry| entry["route"] != "/ready"),
        "/ready should not be in the ring: {items:?}"
    );
}

#[test]
fn ready_is_documented_in_openapi() {
    let document = serde_json::to_value(openapi::document()).unwrap();
    let ready = &document["paths"]["/ready"]["get"];
    assert!(
        ready.is_object(),
        "missing get /ready from the API contract"
    );
    // 公開端點：不要求登入
    assert_eq!(ready["security"], json!([{}]));
    assert!(ready["responses"]["200"].is_object());
    assert!(
        ready["responses"]["503"]["description"]
            .as_str()
            .unwrap()
            .contains("database_unavailable")
    );
}
