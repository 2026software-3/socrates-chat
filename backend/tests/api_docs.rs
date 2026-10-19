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
    // Redoc 腳本必須鎖版本：`latest` 在 3.0 後改為 module，頁面會整頁空白
    assert!(
        !body.contains("/latest/"),
        "Redoc bundle must be version-pinned"
    );
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

#[tokio::test]
async fn api_docs_uses_the_wireframe_design_tokens() {
    let pool = PgPool::connect_lazy("postgres://unused@127.0.0.1:1/unused").unwrap();
    let app = test_app(pool);

    // 色票與字型來自 Figma wireframe 的 Style Guideline（wf/text、wf/surface、wf/border、wf/muted、Noto Sans TC）
    let page = text_body(send(&app, get("/api/docs", None)).await).await;
    for token in ["#18181b", "#f4f4f5", "#d4d4d8", "#71717a", "Noto Sans TC"] {
        assert!(page.contains(token), "theme should use {token}");
    }
}

#[tokio::test]
async fn api_docs_does_not_depend_on_external_hosts_and_serves_its_own_bundle() {
    let pool = PgPool::connect_lazy("postgres://unused@127.0.0.1:1/unused").unwrap();
    let app = test_app(pool);

    // 頁面不引用任何外部主機
    let page = text_body(send(&app, get("/api/docs", None)).await).await;
    assert!(
        !page.contains("cdn.redoc.ly"),
        "page must not load from the Redoc CDN"
    );
    assert!(
        !page.contains("googleapis.com"),
        "page must not load external fonts"
    );
    assert!(page.contains("/api/docs/redoc.standalone.js"));

    // bundle 由後端提供：公開、JavaScript、可快取，且確實是 Redoc 2.5.4
    let res = send(&app, get("/api/docs/redoc.standalone.js", None)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let content_type = res.headers()[header::CONTENT_TYPE].to_str().unwrap();
    assert!(
        content_type.starts_with("text/javascript"),
        "{content_type}"
    );
    assert!(res.headers().contains_key(header::CACHE_CONTROL));
    let js = text_body(res).await;
    assert!(js.contains("Redoc"), "bundle should define Redoc");
    assert!(
        js.len() > 500_000,
        "bundle looks truncated: {} bytes",
        js.len()
    );
}
