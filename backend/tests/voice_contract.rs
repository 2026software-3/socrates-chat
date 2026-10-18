//! S-05.4 語音辨識 API 契約。

use socrates_chat_backend::openapi;

#[test]
fn transcription_documents_binary_audio_request_and_public_response_contract() {
    let document = serde_json::to_value(openapi::document()).unwrap();
    let operation = &document["paths"]["/api/voice/transcribe"]["post"];

    let request = &operation["requestBody"];
    assert_eq!(request["required"], true);
    assert!(request["content"]["audio/*"].is_object());
    assert_eq!(request["content"]["audio/*"]["schema"]["type"], "string");
    assert_eq!(request["content"]["audio/*"]["schema"]["format"], "binary");

    assert_eq!(
        operation["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/Transcript"
    );
    for status in ["400", "401", "403", "502", "503"] {
        assert!(
            operation["responses"][status].is_object(),
            "missing documented response status {status}"
        );
    }
}
