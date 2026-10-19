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

#[tokio::test]
async fn openapi_describes_parameters_and_errors_accurately() {
    let pool = PgPool::connect_lazy("postgres://unused@127.0.0.1:1/unused").unwrap();
    let app = test_app(pool);
    let doc = json_body(send(&app, get("/api/openapi.json", None)).await).await;
    let ops = &doc["paths"];

    // 這些參數沒有出現在路徑裡，必須是 query
    for (path, method) in [
        ("/api/auth/google/callback", "get"),
        ("/api/voice/transcribe", "post"),
    ] {
        for p in ops[path][method]["parameters"].as_array().unwrap() {
            assert_eq!(p["in"], "query", "{path} {}", p["name"]);
        }
    }

    // 語音：本文與回應是二進位，不是整數陣列
    let audio = &doc["components"]["schemas"]["AudioBytes"];
    assert_eq!(
        (&audio["type"], &audio["format"]),
        (&"string".into(), &"binary".into())
    );
    assert!(ops["/api/voice/transcribe"]["post"]["requestBody"]["content"]["audio/*"].is_object());

    // 共通錯誤一律 $ref，公開端點沒有 401／404
    let err =
        &ops["/api/topics"]["get"]["responses"]["401"]["content"]["application/json"]["schema"];
    assert_eq!(err["$ref"], "#/components/schemas/ErrorBody");
    let callback = &ops["/api/auth/google/callback"]["get"]["responses"];
    assert!(callback["401"].is_null() && callback["404"].is_null());

    // 對話相關的端點都在 conversations 下
    for path in [
        "/api/conversations/{id}/end",
        "/api/conversations/{id}/summary",
    ] {
        let tags = ops[path].as_object().unwrap().values().next().unwrap()["tags"].clone();
        assert_eq!(tags, serde_json::json!(["conversations"]), "{path}");
    }
}
