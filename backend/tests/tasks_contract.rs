//! F-19.3 API 契約，不需連線至資料庫。
mod common;

use common::*;
use sqlx::PgPool;

#[tokio::test]
async fn task_endpoints_publish_requests_responses_and_auth_errors() {
    let app = test_app(PgPool::connect_lazy("postgres://unused@127.0.0.1:1/unused").unwrap());
    let doc = json_body(send(&app, get("/api/openapi.json", None)).await).await;
    let paths = &doc["paths"];
    assert!(paths["/api/tasks"]["get"]["responses"]["200"].is_object());
    let claim = &paths["/api/tasks/{id}/claim"]["post"];
    assert_eq!(
        claim["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/Claim"
    );
    for status in ["401", "403", "404", "409", "500"] {
        assert_eq!(
            claim["responses"][status]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/ErrorBody"
        );
    }
    assert!(paths["/api/admin/tasks"]["get"]["responses"]["200"].is_object());
    assert!(paths["/api/admin/tasks/{id}"]["patch"]["requestBody"].is_object());
    let props = &doc["components"]["schemas"]["TaskView"]["properties"];
    for field in [
        "id",
        "period",
        "goal",
        "reward",
        "period_start",
        "progress",
        "claimed",
    ] {
        assert!(props[field].is_object(), "TaskView must document {field}");
    }
}
