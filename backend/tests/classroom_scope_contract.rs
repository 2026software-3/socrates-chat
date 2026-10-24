//! F-28.3 活動與對話歸屬教室的 API 契約：請求與回應格式（教室以明確的參數傳遞）。

use serde_json::Value;
use socrates_chat_backend::openapi;

fn document() -> Value {
    serde_json::to_value(openapi::document()).unwrap()
}

fn query_param<'a>(operation: &'a Value, name: &str) -> Option<&'a Value> {
    operation["parameters"]
        .as_array()?
        .iter()
        .find(|p| p["name"] == name && p["in"] == "query")
}

#[test]
fn activities_carry_their_classroom() {
    let doc = document();
    let new = &doc["components"]["schemas"]["NewActivity"]["properties"];
    assert!(
        new.get("classroom_id").is_some(),
        "NewActivity.classroom_id"
    );
    let activity = &doc["components"]["schemas"]["Activity"]["properties"];
    assert!(
        activity.get("classroom_id").is_some(),
        "Activity.classroom_id"
    );
    // 教室建立後不可更改：PATCH 沒有這個欄位
    let patch = &doc["components"]["schemas"]["ActivityPatch"]["properties"];
    assert!(patch.get("classroom_id").is_none());

    let create = &doc["paths"]["/api/activities"]["post"];
    let codes = create["responses"]["400"]["description"].as_str().unwrap();
    assert!(codes.contains("invalid_classroom"));
    assert!(create["responses"]["404"].is_object());
}

#[test]
fn conversations_expose_a_nullable_classroom() {
    let doc = document();
    let conversation = &doc["components"]["schemas"]["Conversation"]["properties"];
    let field = &conversation["classroom_id"];
    assert!(field.is_object(), "Conversation.classroom_id");
    // 個人練習為 null
    let nullable = field["type"]
        .as_array()
        .is_some_and(|t| t.iter().any(|t| t == "null"))
        || field["oneOf"].is_array();
    assert!(
        nullable,
        "classroom_id must allow null for personal practice"
    );
    let start = &doc["paths"]["/api/conversations"]["post"];
    assert!(
        start["responses"]["400"]["description"]
            .as_str()
            .unwrap()
            .contains("invalid_conversation_source")
    );
}

#[test]
fn teacher_side_lists_take_an_explicit_classroom() {
    let doc = document();
    for (path, method) in [
        ("/api/activities", "get"),
        ("/api/teacher/summaries", "get"),
        ("/api/dashboard/class", "get"),
    ] {
        let operation = &doc["paths"][path][method];
        assert!(
            query_param(operation, "classroom_id").is_some(),
            "{method} {path} must accept ?classroom_id="
        );
        assert!(
            operation["responses"]["404"].is_object(),
            "{method} {path} 404"
        );
    }
    let row = &doc["components"]["schemas"]["TeacherRow"]["properties"];
    assert!(row.get("classroom_id").is_some(), "TeacherRow.classroom_id");
}
