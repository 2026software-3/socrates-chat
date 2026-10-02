//! OpenAiProvider 對本機假伺服器的測試：請求格式、串流解析、錯誤狀態（不呼叫真的 OpenAI）。

use std::sync::{Arc, Mutex};

use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};
use futures_util::StreamExt;
use serde_json::Value;
use socrates_chat_backend::ai::{AiProvider, AiRequest, ChatRole, ChatTurn, OpenAiProvider};

type Captured = Arc<Mutex<Vec<Value>>>;

async fn spawn_server(status: StatusCode, body: &'static str) -> (String, Captured) {
    let captured: Captured = Arc::default();
    let app = Router::new()
        .route(
            "/chat/completions",
            post(
                move |State(c): State<Captured>, Json(v): Json<Value>| async move {
                    c.lock().unwrap().push(v);
                    (status, body).into_response()
                },
            ),
        )
        .with_state(captured.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), captured)
}

fn request() -> AiRequest {
    AiRequest {
        system: "sys".into(),
        messages: vec![
            ChatTurn {
                role: ChatRole::User,
                content: "u1".into(),
            },
            ChatTurn {
                role: ChatRole::Assistant,
                content: "a1".into(),
            },
        ],
    }
}

#[tokio::test]
async fn streams_text_chunks_and_sends_expected_request() {
    let body = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n\
                data: {\"choices\":[{\"delta\":{\"content\":\"你\"}}]}\n\n\
                data: {\"choices\":[{\"delta\":{\"content\":\"好\"}}]}\n\n\
                data: [DONE]\n\n";
    let (base, captured) = spawn_server(StatusCode::OK, body).await;
    let p = OpenAiProvider::new(&base, "k", "some-model");
    let chunks: Vec<String> = p
        .stream_chat(request())
        .await
        .unwrap()
        .map(|c| c.unwrap())
        .collect()
        .await;
    assert_eq!(chunks.concat(), "你好");

    let sent = captured.lock().unwrap()[0].clone();
    assert_eq!(sent["model"], "some-model");
    assert_eq!(sent["stream"], true);
    let roles: Vec<&str> = sent["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles, ["system", "user", "assistant"]);
}

#[tokio::test]
async fn error_status_becomes_ai_error() {
    let (base, _) = spawn_server(StatusCode::INTERNAL_SERVER_ERROR, "boom").await;
    let p = OpenAiProvider::new(&base, "k", "m");
    assert!(p.stream_chat(request()).await.is_err());
}

#[tokio::test]
async fn unreachable_server_becomes_ai_error() {
    let p = OpenAiProvider::new("http://127.0.0.1:1", "k", "m");
    assert!(p.stream_chat(request()).await.is_err());
}
