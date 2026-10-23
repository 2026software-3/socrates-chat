//! F-28.2 修課名單 API 契約：路徑、方法與回應格式（教室為路徑的一部分）。

use socrates_chat_backend::openapi;

#[test]
fn roster_endpoints_live_under_the_classroom() {
    let document = serde_json::to_value(openapi::document()).unwrap();
    let paths = document["paths"].as_object().unwrap();

    for (path, method) in [
        ("/api/classrooms/{id}/roster", "get"),
        ("/api/classrooms/{id}/roster/import", "post"),
        ("/api/classrooms/{id}/roster/{email}", "delete"),
        ("/api/classrooms/{id}/students", "get"),
        ("/api/students/{email}", "delete"),
    ] {
        assert!(
            paths.get(path).and_then(|p| p.get(method)).is_some(),
            "missing {method} {path} from the API contract"
        );
    }
    for path in [
        "/api/roster",
        "/api/roster/import",
        "/api/roster/{email}",
        "/api/students",
    ] {
        assert!(
            paths.get(path).is_none(),
            "{path} is not classroom-scoped and must be gone"
        );
    }
}

#[test]
fn roster_import_contract_reports_added_existing_and_invalid() {
    let document = serde_json::to_value(openapi::document()).unwrap();
    let import = &document["paths"]["/api/classrooms/{id}/roster/import"]["post"];

    assert_eq!(
        import["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/ImportBody"
    );
    assert_eq!(
        import["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/ImportResult"
    );
    let result = &document["components"]["schemas"]["ImportResult"]["properties"];
    for field in ["added", "existing", "invalid", "credentials"] {
        assert!(result.get(field).is_some(), "ImportResult.{field}");
    }
    let params = import["parameters"].as_array().unwrap();
    assert!(
        params
            .iter()
            .any(|p| p["name"] == "id" && p["in"] == "path")
    );
    assert!(import["responses"]["404"].is_object());
    assert!(import["responses"]["403"].is_object());
}

#[test]
fn roster_and_students_list_shapes() {
    let document = serde_json::to_value(openapi::document()).unwrap();
    let roster = &document["paths"]["/api/classrooms/{id}/roster"]["get"];
    assert_eq!(
        roster["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/EmailList"
    );
    let students = &document["paths"]["/api/classrooms/{id}/students"]["get"];
    assert_eq!(
        students["responses"]["200"]["content"]["application/json"]["schema"]["items"]["$ref"],
        "#/components/schemas/StudentRow"
    );
    let row = &document["components"]["schemas"]["StudentRow"]["properties"];
    for field in [
        "id",
        "email",
        "display_name",
        "has_account",
        "disabled",
        "completed_conversations",
    ] {
        assert!(row.get(field).is_some(), "StudentRow.{field}");
    }
}
