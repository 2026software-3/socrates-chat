//! OpenAPI 文件（`GET /api/openapi.json`）與實際路由必須一致。

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test]
async fn every_documented_operation_is_routed_and_protected(pool: PgPool) {
    let app = test_app(pool);
    let res = send(&app, get("/api/openapi.json", None)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let doc = json_body(res).await;
    assert_eq!(doc["openapi"].as_str().unwrap()[..1], *"3");

    let mut count = 0;
    for (path, item) in doc["paths"].as_object().unwrap() {
        for (method, op) in item.as_object().unwrap() {
            count += 1;
            let url = path
                .replace("{id}", "00000000-0000-0000-0000-000000000000")
                .replace("{email}", "someone@example.com");
            let req = if method == "get" {
                get(&url, None)
            } else {
                json_req(&method.to_uppercase(), &url, None, Some(json!({})))
            };
            let status = send(&app, req).await.status();
            // 文件說需要登入（沒有 `security: []`）的端點，未登入必須是 401；
            // 公開端點至少要有對應的路由（不是 404／405）
            let public = op["security"].as_array().is_some_and(|s| {
                s.iter()
                    .any(|r| r.as_object().is_some_and(|o| o.is_empty()))
            });
            if public {
                assert!(
                    status != StatusCode::NOT_FOUND && status != StatusCode::METHOD_NOT_ALLOWED,
                    "{method} {path} is documented but not routed"
                );
            } else {
                assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}");
            }
        }
    }
    assert!(count >= 40, "operations documented: {count}");
}

#[sqlx::test]
async fn documents_the_session_cookie_and_error_shape(pool: PgPool) {
    let app = test_app(pool);
    let doc = json_body(send(&app, get("/api/openapi.json", None)).await).await;
    assert_eq!(
        doc["components"]["securitySchemes"]["session"]["in"],
        "cookie"
    );
    assert_eq!(
        doc["components"]["securitySchemes"]["session"]["name"],
        "sid"
    );
    assert!(doc["components"]["schemas"]["ErrorBody"].is_object());
}

#[sqlx::test]
async fn student_deletion_documents_delete_with_confirmation_body(pool: PgPool) {
    let app = test_app(pool);
    let doc = json_body(send(&app, get("/api/openapi.json", None)).await).await;
    let operation = &doc["paths"]["/api/students/{email}"]["delete"];
    assert!(operation.is_object());
    assert_eq!(operation["requestBody"]["required"], true);
    assert_eq!(
        operation["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/DeleteBody"
    );
    assert!(operation["responses"]["204"].is_object());
    assert!(doc["paths"].get("/api/students/{email}/delete").is_none());
}

/// 分頁列表都要記載 limit／cursor、400，並以 `$ref` 指向具名的分頁 schema。
#[sqlx::test]
async fn paginated_lists_document_limit_cursor_and_page_schema(pool: PgPool) {
    let app = test_app(pool);
    let doc = json_body(send(&app, get("/api/openapi.json", None)).await).await;
    for (path, schema) in [
        ("/api/conversations", "ConversationPage"),
        ("/api/students", "StudentPage"),
        ("/api/roster", "RosterPage"),
        ("/api/activities", "ActivityPage"),
    ] {
        let op = &doc["paths"][path]["get"];
        let params: Vec<&str> = op["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert_eq!(params, ["limit", "cursor"], "{path}");
        assert_eq!(
            op["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
            format!("#/components/schemas/{schema}"),
            "{path}"
        );
        assert!(op["responses"]["400"].is_object(), "{path}");
        assert!(
            doc["components"]["schemas"][schema]["properties"]["next_cursor"].is_object(),
            "{path}"
        );
    }
}
