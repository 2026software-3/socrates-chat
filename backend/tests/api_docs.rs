//! API reference 頁面（`GET /api/docs`，Redoc）由 OpenAPI 文件渲染，公開且不需登入。

mod common;

use axum::http::{StatusCode, header};
use common::*;
use sqlx::PgPool;

// 這個頁面不碰資料庫，所以用惰性連線，不需要啟動 PostgreSQL。
#[tokio::test]
async fn api_docs_page_is_public_html_rendered_from_openapi() {
    let pool = PgPool::connect_lazy("postgres://unused@127.0.0.1:1/unused").unwrap();
    let app = test_app(pool);
    let res = send(&app, get("/api/docs", None)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let content_type = res.headers()[header::CONTENT_TYPE].to_str().unwrap();
    assert!(content_type.starts_with("text/html"), "{content_type}");
    let body = text_body(res).await;
    assert!(
        body.contains("Socrates Chat API"),
        "page should embed the spec"
    );
}
