//! F-19.4 API 契約，不需連線至資料庫。
mod common;
use common::*;
use sqlx::PgPool;

#[tokio::test]
async fn purchase_endpoints_document_auth_and_errors() {
    let app = test_app(PgPool::connect_lazy("postgres://unused@127.0.0.1:1/unused").unwrap());
    let doc = json_body(send(&app, get("/api/openapi.json", None)).await).await;
    let paths = &doc["paths"];
    assert!(paths["/api/parts"]["get"]["responses"]["200"].is_object());
    let purchase = &paths["/api/parts/{id}/purchase"]["post"];
    for status in ["401", "403", "404", "409", "500"] {
        assert_eq!(
            purchase["responses"][status]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/ErrorBody"
        );
    }
    assert!(paths["/api/admin/parts"]["get"]["responses"]["200"].is_object());
    assert!(paths["/api/admin/parts/{id}"]["patch"]["requestBody"].is_object());
    let props = &doc["components"]["schemas"]["PartView"]["properties"];
    for field in [
        "id",
        "name_zh_tw",
        "name_en",
        "name_es",
        "price",
        "enabled",
        "owned",
    ] {
        assert!(props[field].is_object(), "PartView must document {field}");
    }
}
