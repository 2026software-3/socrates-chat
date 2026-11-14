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

/// Vite 產物的檔名帶內容雜湊，可以長期快取；`index.html` 每次都要重新驗證，部署後才拿得到新的檔名。
#[tokio::test]
async fn hashed_assets_cache_forever_and_pages_revalidate() {
    let (app, dir) = app_with_frontend();
    let res = send(&app, get("/assets/app.js", None)).await;
    assert_eq!(
        res.headers()[header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );
    for path in ["/", "/index.html", "/conversations/abc"] {
        let res = send(&app, get(path, None)).await;
        assert_eq!(res.headers()[header::CACHE_CONTROL], "no-cache", "{path}");
    }
    std::fs::remove_dir_all(dir).ok();
}

/// 重新部署後舊分頁要的 chunk 已不存在：回 404，不能把 `index.html` 當成 JS 回去，更不能讓它被長期快取。
#[tokio::test]
async fn a_missing_asset_is_404_not_the_spa() {
    let (app, dir) = app_with_frontend();
    let res = send(&app, get("/assets/gone-abc123.js", None)).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(res.headers()[header::CACHE_CONTROL], "no-cache");
    assert!(!text_body(res).await.contains("<title>SPA</title>"));
    std::fs::remove_dir_all(dir).ok();
}

/// 建置時預先壓好的 `.gz`（Dockerfile）：瀏覽器接受 gzip 時直接送它，否則送原檔；沒有 `.gz` 的檔案照常送原檔。
#[tokio::test]
async fn serves_the_precompressed_file_when_the_client_accepts_gzip() {
    let (app, dir) = app_with_frontend();
    std::fs::write(dir.join("assets/app.js.gz"), "precompressed").unwrap();
    let gzip = |path: &str| {
        let mut req = get(path, None);
        req.headers_mut()
            .insert(header::ACCEPT_ENCODING, "gzip, br".parse().unwrap());
        req
    };

    let res = send(&app, gzip("/assets/app.js")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CONTENT_ENCODING], "gzip");
    assert_eq!(res.headers()[header::VARY], "accept-encoding");
    assert!(
        res.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .contains("javascript")
    );
    assert_eq!(text_body(res).await, "precompressed");

    let res = send(&app, get("/assets/app.js", None)).await;
    assert!(!res.headers().contains_key(header::CONTENT_ENCODING));
    assert_eq!(text_body(res).await, "console.log('hi')");

    // index.html 沒有 .gz：接受 gzip 也送原檔
    let res = send(&app, gzip("/")).await;
    assert!(!res.headers().contains_key(header::CONTENT_ENCODING));
    assert!(text_body(res).await.contains("<title>SPA</title>"));
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
