//! OpenAPI 文件（`GET /api/openapi.json`）與實際路由必須一致。

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test]
async fn every_documented_operation_is_routed_and_protected(pool: PgPool) {
    // 示範帳號端點只在 DEMO_MODE 下存在（否則 404），所以用示範模式檢查路由
    let config = socrates_chat_backend::config::Config {
        demo_mode: true,
        ..test_config()
    };
    let app = test_app_with_config(pool, FakeAi::with(vec![]), config);
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
    for (path, schema, expected) in [
        (
            "/api/conversations",
            "ConversationPage",
            vec!["limit", "cursor", "archived", "activity_id"],
        ),
        (
            "/api/classrooms/{id}/students",
            "StudentPage",
            vec!["id", "limit", "cursor", "q"],
        ),
        (
            "/api/classrooms/{id}/roster",
            "RosterPage",
            vec!["id", "limit", "cursor"],
        ),
        (
            "/api/activities",
            "ActivityPage",
            vec!["limit", "cursor", "classroom_id", "archived"],
        ),
    ] {
        let op = &doc["paths"][path]["get"];
        let params: Vec<&str> = op["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert_eq!(params, expected, "{path}");
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

/// 學生列表的每一頁都帶符合 `q` 的 `total`，且 `q` 有 `invalid_filter` 錯誤說明。
#[sqlx::test]
async fn student_page_documents_total_and_invalid_filter(pool: PgPool) {
    let app = test_app(pool);
    let doc = json_body(send(&app, get("/api/openapi.json", None)).await).await;
    let page = &doc["components"]["schemas"]["StudentPage"];
    assert!(page["properties"]["total"].is_object());
    assert!(
        page["required"]
            .as_array()
            .unwrap()
            .contains(&json!("total"))
    );
    let op = &doc["paths"]["/api/classrooms/{id}/students"]["get"];
    assert!(
        op["responses"]["400"]["description"]
            .as_str()
            .unwrap()
            .contains("invalid_filter")
    );
}

/// 文件列出的 409 錯誤碼必須是端點實際可能回的：新建教室不可能已封存；
/// 開始對話遇到封存的活動或教室回 400 `invalid_conversation_source`，不是 409。
#[sqlx::test]
async fn documented_conflicts_are_ones_the_endpoint_can_return(pool: PgPool) {
    let app = test_app(pool);
    let doc = json_body(send(&app, get("/api/openapi.json", None)).await).await;
    let conflict = |path: &str, method: &str| -> String {
        doc["paths"][path][method]["responses"]["409"]["description"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };
    assert_eq!(conflict("/api/classrooms", "post"), "classroom_name_taken");
    assert_eq!(
        conflict("/api/conversations", "post"),
        "activity_already_started"
    );
    let start = doc["paths"]["/api/conversations"]["post"]["responses"]["400"]["description"]
        .as_str()
        .unwrap();
    assert!(start.contains("invalid_conversation_source") && start.contains("已封存"));
    // 真正會被封存擋下的寫入端點仍要標示 classroom_archived
    for (path, method) in [
        ("/api/activities", "post"),
        ("/api/classrooms/{id}", "patch"),
        ("/api/classrooms/{id}/roster/import", "post"),
        ("/api/conversations/{id}/end", "post"),
    ] {
        assert!(
            conflict(path, method).contains("classroom_archived"),
            "{method} {path}"
        );
    }
}
