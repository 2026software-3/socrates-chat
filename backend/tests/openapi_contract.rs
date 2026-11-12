//! OpenAPI 契約測試：各功能的路徑、請求與回應格式（原本分散在四個測試檔，合併以減少編譯與連結單位）。

mod classroom {
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

        let request_schema = &document["components"]["schemas"]["NewClassroom"]["properties"];
        assert_eq!(request_schema["name"]["maxLength"], 255);
        assert_eq!(request_schema["year_label"]["maxLength"], 100);
        assert_eq!(request_schema["description"]["maxLength"], 2000);
        assert!(
            create["responses"]["400"]["description"]
                .as_str()
                .unwrap()
                .contains("invalid_classroom_description")
        );
    }
}

mod classroom_scope {
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
    fn activities_contract_supports_multiple_topics() {
        let doc = document();
        let new = &doc["components"]["schemas"]["NewActivity"]["properties"];
        assert!(new.get("topic_ids").is_some(), "NewActivity.topic_ids");
        let patch = &doc["components"]["schemas"]["ActivityPatch"]["properties"];
        assert!(patch.get("topic_ids").is_some(), "ActivityPatch.topic_ids");
        let activity = &doc["components"]["schemas"]["Activity"]["properties"];
        assert!(activity.get("topic_ids").is_some(), "Activity.topic_ids");

        let patch_op = &doc["paths"]["/api/activities/{id}"]["patch"];
        let conflicts = patch_op["responses"]["409"]["description"]
            .as_str()
            .unwrap();
        assert!(conflicts.contains("activity_topic_locked"));
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
    fn conversation_creation_documents_activity_topic_selection() {
        let doc = document();
        let schema = &doc["components"]["schemas"]["NewConversation"]["properties"];
        assert!(schema["activity_id"].is_object());
        assert!(
            schema["topic_id"]["description"]
                .as_str()
                .unwrap()
                .contains("活動")
        );
        let operation = &doc["paths"]["/api/conversations"]["post"];
        assert!(
            operation["responses"]["400"]["description"]
                .as_str()
                .unwrap()
                .contains("題目不屬於活動")
        );
        assert!(
            operation["responses"]["409"]["description"]
                .as_str()
                .unwrap()
                .contains("activity_already_started")
        );
    }

    #[test]
    fn teacher_side_lists_take_an_explicit_classroom() {
        let doc = document();
        for (path, method) in [
            ("/api/activities", "get"),
            ("/api/summaries", "get"),
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
        let row = &doc["components"]["schemas"]["SummaryRow"]["properties"];
        assert!(row.get("classroom_id").is_some(), "SummaryRow.classroom_id");
    }
}

mod roster {
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
        for field in ["added", "existing", "errors", "invalid", "credentials"] {
            assert!(result.get(field).is_some(), "ImportResult.{field}");
        }
        let credential = &document["components"]["schemas"]["Credential"]["properties"];
        assert!(credential.get("student_number").is_some());
        assert!(credential.get("temporary_password").is_some());
        let error = &document["components"]["schemas"]["StudentImportError"]["properties"];
        assert!(error.get("line").is_some());
        assert!(error.get("reason").is_some());
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
            "#/components/schemas/RosterPage"
        );
        let students = &document["paths"]["/api/classrooms/{id}/students"]["get"];
        assert_eq!(
            students["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/StudentPage"
        );
        assert_eq!(
            document["components"]["schemas"]["StudentPage"]["properties"]["items"]["items"]["$ref"],
            "#/components/schemas/StudentRow"
        );
        let row = &document["components"]["schemas"]["StudentRow"]["properties"];
        for field in [
            "id",
            "email",
            "student_number",
            "display_name",
            "has_account",
            "disabled",
            "completed_conversations",
        ] {
            assert!(row.get(field).is_some(), "StudentRow.{field}");
        }
    }

    /// #446：學號叫 `student_number`，學生的使用者 UUID 叫 `student_user_id`，API 不再有 `student_id`。
    #[test]
    fn student_identifiers_are_named_apart() {
        let document = serde_json::to_value(openapi::document()).unwrap();
        let schemas = &document["components"]["schemas"];
        for (schema, field) in [
            ("StudentRow", "student_number"),
            ("Credential", "student_number"),
            ("DeleteBody", "confirm_student_number"),
            ("SummaryRow", "student_user_id"),
            ("ConversationTranscript", "student_user_id"),
        ] {
            assert!(
                schemas[schema]["properties"].get(field).is_some(),
                "{schema}.{field}"
            );
        }
        let text = document.to_string();
        assert!(
            !text.contains("\"student_id\""),
            "student_id left in OpenAPI"
        );
        assert!(!text.contains("confirm_student_id"));
    }

    /// F-30.2：名單、教師總結列表與 `/api/me` 都帶學號。
    #[test]
    fn roster_summaries_and_me_carry_the_student_number() {
        let document = serde_json::to_value(openapi::document()).unwrap();
        let schemas = &document["components"]["schemas"];
        for (schema, field) in [
            ("RosterEntry", "student_number"),
            ("RosterEntry", "display_name"),
            ("SummaryRow", "student_number"),
            ("MeResponse", "student_number"),
        ] {
            assert!(
                schemas[schema]["properties"].get(field).is_some(),
                "{schema}.{field}"
            );
        }
    }
}

mod voice {
    //! S-05.4 語音辨識 API 契約。

    use socrates_chat_backend::openapi;

    #[test]
    fn transcription_documents_request_response_and_error_contracts() {
        let document = serde_json::to_value(openapi::document()).unwrap();
        let operation = &document["paths"]["/api/voice/transcribe"]["post"];

        let request = &operation["requestBody"];
        assert_eq!(request["required"], true);
        assert!(request["content"]["audio/*"].is_object());
        let audio_schema = &request["content"]["audio/*"]["schema"];
        let audio_schema = audio_schema["$ref"]
            .as_str()
            .and_then(|reference| reference.strip_prefix("#/components/schemas/"))
            .map_or(audio_schema, |name| {
                &document["components"]["schemas"][name]
            });
        assert_eq!(audio_schema["type"], "string");
        assert_eq!(audio_schema["format"], "binary");

        let language = operation["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .find(|parameter| parameter["name"] == "language")
            .expect("language query parameter should be documented");
        assert_eq!(language["in"], "query");
        assert_eq!(language["required"], true);
        assert_eq!(language["schema"]["type"], "string");

        assert_eq!(
            operation["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/Transcript"
        );
        let transcript = &document["components"]["schemas"]["Transcript"];
        assert_eq!(transcript["properties"]["text"]["type"], "string");

        for status in ["400", "413", "502", "503"] {
            assert!(
                operation["responses"][status].is_object(),
                "missing response {status}"
            );
            assert_eq!(
                operation["responses"][status]["content"]["application/json"]["schema"]["$ref"],
                "#/components/schemas/ErrorBody",
                "response {status} should use ErrorBody"
            );
        }
        for status in ["401", "403"] {
            assert!(
                operation["responses"][status].is_object(),
                "missing common response {status}"
            );
        }

        let bad_request_description = operation["responses"]["400"]["description"]
            .as_str()
            .unwrap();
        assert!(bad_request_description.contains("invalid_language"));
        assert!(bad_request_description.contains("invalid_audio"));
        let payload_too_large_description = operation["responses"]["413"]["description"]
            .as_str()
            .unwrap();
        assert!(payload_too_large_description.contains("10 MB"));
    }
}

#[test]
fn dashboard_contract_scopes_distributions_and_regroup_to_activity_topics() {
    let doc = serde_json::to_value(socrates_chat_backend::openapi::document()).unwrap();
    let schemas = &doc["components"]["schemas"];
    let activity = &schemas["ActivityDistribution"]["properties"];
    assert!(activity["topics"].is_object());
    assert!(activity.get("claims").is_none());
    let topic = &schemas["TopicDistribution"]["properties"];
    for field in ["topic_id", "title", "completed", "claims", "ungrouped"] {
        assert!(
            topic[field].is_object(),
            "missing TopicDistribution.{field}"
        );
    }
    let required = schemas["RegroupBody"]["required"].as_array().unwrap();
    assert!(required.contains(&serde_json::json!("activity_id")));
    assert!(required.contains(&serde_json::json!("topic_id")));
}
