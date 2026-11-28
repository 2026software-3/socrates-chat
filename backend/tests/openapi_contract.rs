//! OpenAPI 契約測試：各功能的路徑、請求與回應格式（原本分散在四個測試檔，合併以減少編譯與連結單位）。

mod common;

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
        assert_eq!(
            response_schema["$ref"],
            "#/components/schemas/ClassroomDetail"
        );
        let update = &paths["/api/classrooms/{id}"]["patch"];
        assert_eq!(
            update["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/ClassroomDetail"
        );

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

    /// F-29.3：總結列帶有活動完成狀態與題目、活動 ID。
    #[test]
    fn summary_rows_contract_supports_activity_completion() {
        let doc = document();
        let row = &doc["components"]["schemas"]["SummaryRow"]["properties"];
        assert!(
            row.get("activity_completed").is_some(),
            "SummaryRow.activity_completed"
        );
        assert!(row.get("activity_id").is_some(), "SummaryRow.activity_id");
        assert!(row.get("topic_id").is_some(), "SummaryRow.topic_id");
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

#[test]
fn radar_contract_exposes_status_retry_and_scores_without_the_rubric() {
    let doc = serde_json::to_value(socrates_chat_backend::openapi::document()).unwrap();
    let view = "#/components/schemas/RadarView";
    let read = &doc["paths"]["/api/conversations/{id}/radar"]["get"];
    assert_eq!(
        read["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        view
    );
    let retry = &doc["paths"]["/api/conversations/{id}/radar/retry"]["post"];
    assert_eq!(
        retry["responses"]["202"]["content"]["application/json"]["schema"]["$ref"],
        view
    );
    let conflicts = retry["responses"]["409"]["description"].as_str().unwrap();
    for code in [
        "classroom_archived",
        "dimensions_pending",
        "radar_not_retryable",
    ] {
        assert!(conflicts.contains(code), "missing 409 {code}");
    }
    let schemas = &doc["components"]["schemas"];
    for field in [
        "status",
        "retryable",
        "scores",
        "dimensions",
        "rules_version",
        "completed_at",
    ] {
        assert!(
            schemas["RadarView"]["properties"][field].is_object(),
            "missing RadarView.{field}"
        );
    }
    let statuses = schemas["RadarStatus"].to_string();
    for status in [
        "summary_not_ready",
        "unavailable",
        "dimensions_pending",
        "not_started",
        "pending",
        "failed",
        "ready",
    ] {
        assert!(statuses.contains(status), "missing status {status}");
    }
    let dimension = &schemas["RadarDimension"]["properties"];
    assert!(dimension["name"].is_object() && dimension["description"].is_object());
    assert!(dimension.get("rubric").is_none());
}

/// utoipa 以型別名稱註冊 schema；兩個同名的 `ToSchema` 型別在 OpenAPI 裡會互相覆蓋
/// （曾讓 `/api/admin/users/{id}/status` 的請求格式變成客訴的 `StatusBody`），所以名稱不可重複。
mod schema_names {
    use std::{collections::BTreeMap, fs, path::Path};

    /// 形狀完全相同、覆蓋也無妨的同名型別。
    const IDENTICAL: &[&str] = &["Localized"];

    /// 一段原始碼裡，`ToSchema` 型別在 OpenAPI 裡註冊的名稱（`#[schema(as = path::Name)]` 會改名）。
    /// 逐行掃描：認得多行 `#[derive(…)]`、`pub(crate)`／`pub(super)`／`pub(in …)`，不需要額外的解析套件。
    fn schema_names_in(text: &str) -> Vec<String> {
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        let mut names = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let mut declaration = *line;
            if let Some(rest) = declaration.strip_prefix("pub") {
                declaration = rest.trim_start();
                if declaration.starts_with('(') {
                    declaration = declaration
                        .split_once(')')
                        .map_or("", |(_, rest)| rest)
                        .trim_start();
                }
            }
            let Some(rest) = declaration
                .strip_prefix("struct ")
                .or_else(|| declaration.strip_prefix("enum "))
            else {
                continue;
            };
            let ident: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            // 往回收集緊鄰的屬性與註解，直到上一個項目結束
            let mut attributes = String::new();
            for previous in lines[..i].iter().rev() {
                if previous.ends_with('}') || previous.ends_with(';') || previous.ends_with('{') {
                    break;
                }
                attributes.push_str(previous);
                attributes.push('\n');
            }
            if !attributes.contains("ToSchema") {
                continue;
            }
            let renamed = attributes.split("as = ").nth(1).and_then(|after| {
                let path = after.split([')', ',', ' ', '\n']).next()?;
                path.rsplit("::")
                    .next()
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
            });
            names.push(renamed.unwrap_or(ident));
        }
        names
    }

    #[test]
    fn scanner_understands_multiline_derives_and_renames() {
        let source = "\
#[derive(
    Serialize,
    ToSchema,
)]
pub(super) struct A {}

/// doc
#[derive(ToSchema)]
#[schema(as = other::B)]
pub struct C;

// comment
#[derive(Debug)]
struct NotSchema {}

#[derive(utoipa::ToSchema)]
pub(in crate::x) enum D { One }
";
        assert_eq!(schema_names_in(source), ["A", "B", "D"]);
    }

    #[test]
    fn to_schema_type_names_are_unique() {
        let files = super::common::rust_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
        let mut seen: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for file in files {
            let text = fs::read_to_string(&file).unwrap();
            for name in schema_names_in(&text) {
                seen.entry(name)
                    .or_default()
                    .push(file.file_name().unwrap().to_string_lossy().into_owned());
            }
        }
        // 同一個檔案裡不同 `mod` 的同名型別，utoipa 一樣只認名稱，所以同樣算重複
        let duplicated: Vec<_> = seen
            .iter()
            .filter(|(name, files)| files.len() > 1 && !IDENTICAL.contains(&name.as_str()))
            .collect();
        assert!(
            duplicated.is_empty(),
            "同名的 ToSchema 型別會在 OpenAPI 互相覆蓋：{duplicated:?}"
        );
    }
}

/// 整份 OpenAPI 與進版控的快照（`docs/api/openapi.json`）逐項比對。
/// 前端的契約測試（`frontend/src/test/contract.test.ts`）用同一份快照驗證 MSW 假後端的回應，
/// 所以 API 有任何變動，都要明確更新快照，前端才會發現假後端落後。
/// 更新方式：`UPDATE_OPENAPI_SNAPSHOT=1 cargo test --test openapi_contract snapshot`。
mod snapshot {
    use socrates_chat_backend::openapi;
    use std::path::PathBuf;

    #[test]
    fn openapi_document_matches_committed_snapshot() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/api/openapi.json");
        let mut actual = serde_json::to_value(openapi::document()).unwrap();

        if std::env::var_os("UPDATE_OPENAPI_SNAPSHOT").is_some() {
            let mut text = serde_json::to_string_pretty(&actual).unwrap();
            text.push('\n');
            std::fs::write(&path, text).unwrap();
            return;
        }

        let mut committed: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&path).expect("缺少 docs/api/openapi.json 快照"),
        )
        .expect("快照不是合法 JSON");
        // `info.version` 來自 Cargo.toml；改版本號不是 API 變動，不該讓這個測試失敗
        for document in [&mut committed, &mut actual] {
            document["info"]["version"] = serde_json::Value::Null;
        }
        assert!(
            committed == actual,
            "API 契約與 docs/api/openapi.json 不一致；確認變動符合預期後，\
             以 UPDATE_OPENAPI_SNAPSHOT=1 cargo test --test openapi_contract snapshot 更新快照"
        );
    }
}
