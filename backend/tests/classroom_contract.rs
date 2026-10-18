//! F-28.1 classroom API contract coverage.

use serde_json::Value;
use socrates_chat_backend::openapi;

#[test]
fn classrooms_endpoints_are_in_openapi() {
    let document = serde_json::to_value(openapi::document()).unwrap();
    let paths = document["paths"].as_object().unwrap();

    for (path, method) in [
        ("/api/classrooms", "get"),
        ("/api/classrooms", "post"),
        ("/api/classrooms/{id}", "get"),
        ("/api/classrooms/{id}", "patch"),
    ] {
        assert!(
            paths
                .get(path)
                .and_then(|operations| operations.get(method))
                .is_some(),
            "missing {method} {path} from the API contract"
        );
    }

    let create = &paths["/api/classrooms"]["post"];
    assert_eq!(
        create["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/NewClassroom"
    );
    let response_schema: &Value =
        &create["responses"]["201"]["content"]["application/json"]["schema"];
    assert_eq!(response_schema["$ref"], "#/components/schemas/Classroom");
}
