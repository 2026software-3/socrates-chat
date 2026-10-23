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
