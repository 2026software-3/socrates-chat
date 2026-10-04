//! 由 Rust 後端提供前端建置產物（S-08.1：前端與 API 同網域）。

mod common;

use std::sync::Arc;

use axum::{
    Router,
    http::{StatusCode, header},
};
use common::*;
use socrates_chat_backend::{AppState, app};
use sqlx::postgres::PgPoolOptions;

/// 建立含 `index.html` 與 `assets/app.js` 的暫存前端目錄，並回傳掛上它的 app。
fn app_with_frontend() -> (Router, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("socrates-fe-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("index.html"), "<!doctype html><title>SPA</title>").unwrap();
    std::fs::write(dir.join("assets/app.js"), "console.log('hi')").unwrap();

    let mut config = test_config();
    config.frontend_dir = Some(dir.clone());
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://unused@127.0.0.1:1/unused")
        .unwrap();
    let state = AppState::new(pool, config, Arc::new(FakeIdentity), FakeAi::with(vec![]));
    (app(state), dir)
}

#[tokio::test]
async fn serves_index_at_root_and_assets_by_path() {
    let (app, dir) = app_with_frontend();
    let res = send(&app, get("/", None)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(text_body(res).await.contains("<title>SPA</title>"));

    let res = send(&app, get("/assets/app.js", None)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        res.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .contains("javascript")
    );
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn client_side_routes_fall_back_to_index() {
    let (app, dir) = app_with_frontend();
    for path in ["/conversations/abc", "/teacher/roster", "/login"] {
        let res = send(&app, get(path, None)).await;
        assert_eq!(res.status(), StatusCode::OK, "{path}");
        assert!(
            text_body(res).await.contains("<title>SPA</title>"),
            "{path}"
        );
    }
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn unknown_api_paths_stay_json_404_not_the_spa() {
    let (app, dir) = app_with_frontend();
    let res = send(&app, get("/api/does-not-exist", None)).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let body = json_body(res).await;
    assert_eq!(body["error"]["code"], "not_found");
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn api_and_health_still_work_with_frontend_mounted() {
    let (app, dir) = app_with_frontend();
    let res = send(&app, get("/health", None)).await;
    assert_eq!(res.status(), StatusCode::OK);
    // 未登入的 API 仍是 401，而不是被前端吃掉
    let res = send(&app, get("/api/me", None)).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn without_frontend_dir_unknown_paths_are_plain_404() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://unused@127.0.0.1:1/unused")
        .unwrap();
    let app = test_app(pool);
    let res = send(&app, get("/conversations/abc", None)).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
