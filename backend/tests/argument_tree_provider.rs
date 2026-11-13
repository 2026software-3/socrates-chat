// Verify the same typed Responses path used by the saved tree API.
use axum::{Json, Router, http::StatusCode, routing::post};
use serde_json::{Value, json};
use socrates_chat_backend::{
    ai::{AiProvider, OpenAiProvider, StructuredRequest},
    key_pool::KeyPool,
    pii::MaskingAi,
};
use std::sync::Arc;

#[tokio::test]
async fn typed_responses_request_masks_values_before_json_serialization() {
    let app = Router::new().route("/v1/responses", post(|Json(body): Json<Value>| async move {
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["reasoning"]["effort"], "high");
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], false);
        assert_eq!(body["text"]["format"]["strict"], true);
        assert_eq!(body["text"]["format"]["type"], "json_schema");
        assert_eq!(body["text"]["format"]["name"], "test_schema");
        let input: Value = serde_json::from_str(body["input"][0]["content"].as_str().unwrap()).unwrap();
        assert_eq!(input["answer"], "\n[EMAIL]\r\n[PHONE]\t\"C:\\notes\"");
        assert!(!body.to_string().contains("student@example.com"));
        Json(json!({"status":"completed","model":"test-model","output":[
            {"type":"reasoning","summary":[]},
            {"type":"message","status":"completed","role":"assistant","content":[{"type":"output_text","text":"{\"ok\":true}"}]}
        ]}))
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let provider = OpenAiProvider::new(
        &format!("http://{addr}/v1"),
        Arc::new(KeyPool::new(vec!["synthetic-key".into()]).unwrap()),
        "test-model",
    )
    .with_reasoning_effort(Some("high".into()));
    let ai = MaskingAi::new(Arc::new(provider));
    let response = ai.complete_json(StructuredRequest {
        system: "Return JSON".into(),
        input: json!({"answer":"\nstudent@example.com\r\n0912345678\t\"C:\\notes\""}),
        schema: json!({"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"],"additionalProperties":false}),
        name: "test_schema".into(),
    }).await.unwrap();
    assert_eq!(response.content, "{\"ok\":true}");
    assert_eq!(response.model, "test-model");
    task.abort();
}

#[tokio::test]
async fn responses_http_errors_do_not_expose_provider_body() {
    let app = Router::new().route(
        "/v1/responses",
        post(|| async { (StatusCode::BAD_REQUEST, "private provider diagnostic") }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let ai = OpenAiProvider::new(
        &format!("http://{addr}/v1"),
        Arc::new(KeyPool::new(vec!["synthetic-key".into()]).unwrap()),
        "test-model",
    );
    let error = ai
        .complete_json(StructuredRequest {
            system: "JSON".into(),
            input: json!({}),
            schema: json!({"type":"object"}),
            name: "test".into(),
        })
        .await
        .unwrap_err();
    assert!(!format!("{error:?}").contains("private"));
    task.abort();
}
