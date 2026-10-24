//! OpenAiProvider 對本機假伺服器的測試：請求格式、串流解析、錯誤狀態（不呼叫真的 OpenAI）。

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use futures_util::StreamExt;
use serde_json::Value;
use socrates_chat_backend::{
    ai::{AiProvider, AiRequest, ChatRole, ChatTurn, OpenAiProvider},
    key_pool::KeyPool,
};

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

fn single(key: &str) -> Arc<KeyPool> {
    Arc::new(KeyPool::new(vec![key.to_string()]).unwrap())
}

fn pool_of(keys: &[&str]) -> Arc<KeyPool> {
    Arc::new(KeyPool::new(keys.iter().map(|k| k.to_string()).collect()).unwrap())
}

const OK_BODY: &str = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n";

/// 依 `Authorization: Bearer <key>` 回不同結果的假伺服器；同時記錄每次收到的 key。
type Seen = Arc<Mutex<Vec<String>>>;

async fn spawn_keyed_server(
    behaviour: Vec<(&'static str, StatusCode, Option<&'static str>)>,
) -> (String, Seen) {
    #[derive(Clone)]
    struct S {
        by_key: Arc<HashMap<String, (StatusCode, Option<&'static str>)>>,
        seen: Seen,
    }
    let seen: Seen = Arc::default();
    let state = S {
        by_key: Arc::new(
            behaviour
                .into_iter()
                .map(|(k, st, ra)| (format!("Bearer {k}"), (st, ra)))
                .collect(),
        ),
        seen: seen.clone(),
    };
    let app = Router::new()
        .route(
            "/chat/completions",
            post(|State(s): State<S>, h: HeaderMap| async move {
                let auth = h["authorization"].to_str().unwrap().to_string();
                s.seen
                    .lock()
                    .unwrap()
                    .push(auth.trim_start_matches("Bearer ").to_string());
                let (status, retry_after) = s.by_key[&auth];
                let mut res = if status.is_success() {
                    (status, OK_BODY).into_response()
                } else {
                    (status, "nope").into_response()
                };
                if let Some(v) = retry_after {
                    res.headers_mut().insert("retry-after", v.parse().unwrap());
                }
                res
            }),
        )
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), seen)
}

async fn text_of(p: &OpenAiProvider) -> Result<String, ()> {
    let stream = p.stream_chat(request()).await.map_err(|_| ())?;
    let chunks: Vec<_> = stream.collect().await;
    chunks
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .map(|c| c.concat())
        .map_err(|_| ())
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
    let p = OpenAiProvider::new(&base, single("k"), "some-model");
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
    let p = OpenAiProvider::new(&base, single("k"), "m");
    assert!(p.stream_chat(request()).await.is_err());
}

#[tokio::test]
async fn unreachable_server_becomes_ai_error() {
    let p = OpenAiProvider::new("http://127.0.0.1:1", single("k"), "m");
    assert!(p.stream_chat(request()).await.is_err());
}

#[tokio::test]
async fn requests_rotate_across_keys() {
    let (base, seen) = spawn_keyed_server(vec![
        ("a", StatusCode::OK, None),
        ("b", StatusCode::OK, None),
    ])
    .await;
    let p = OpenAiProvider::new(&base, pool_of(&["a", "b"]), "m");
    for _ in 0..4 {
        assert_eq!(text_of(&p).await.unwrap(), "ok");
    }
    assert_eq!(*seen.lock().unwrap(), ["a", "b", "a", "b"]);
}

#[tokio::test]
async fn rate_limited_key_is_replaced_and_cooled_down() {
    let (base, seen) = spawn_keyed_server(vec![
        ("a", StatusCode::TOO_MANY_REQUESTS, Some("30")),
        ("b", StatusCode::OK, None),
    ])
    .await;
    let p = OpenAiProvider::new(&base, pool_of(&["a", "b"]), "m");
    assert_eq!(text_of(&p).await.unwrap(), "ok");
    assert_eq!(*seen.lock().unwrap(), ["a", "b"]);
    // a 還在冷卻，之後的請求不再碰它
    assert_eq!(text_of(&p).await.unwrap(), "ok");
    assert_eq!(text_of(&p).await.unwrap(), "ok");
    assert_eq!(*seen.lock().unwrap(), ["a", "b", "b", "b"]);
}

#[tokio::test]
async fn unauthorized_key_is_disabled_and_replaced() {
    for status in [
        StatusCode::UNAUTHORIZED,
        StatusCode::PAYMENT_REQUIRED,
        StatusCode::FORBIDDEN,
    ] {
        let (base, seen) =
            spawn_keyed_server(vec![("bad", status, None), ("good", StatusCode::OK, None)]).await;
        let pool = pool_of(&["bad", "good"]);
        let p = OpenAiProvider::new(&base, pool.clone(), "m");
        assert_eq!(text_of(&p).await.unwrap(), "ok", "{status}");
        assert_eq!(pool.usable_count(), 1, "{status}");
        assert_eq!(text_of(&p).await.unwrap(), "ok");
        assert_eq!(*seen.lock().unwrap(), ["bad", "good", "good"], "{status}");
    }
}

#[tokio::test]
async fn server_error_retries_on_next_key_without_penalising_the_key() {
    let (base, seen) = spawn_keyed_server(vec![
        ("a", StatusCode::BAD_GATEWAY, None),
        ("b", StatusCode::OK, None),
    ])
    .await;
    let pool = pool_of(&["a", "b"]);
    let p = OpenAiProvider::new(&base, pool.clone(), "m");
    assert_eq!(text_of(&p).await.unwrap(), "ok");
    assert_eq!(*seen.lock().unwrap(), ["a", "b"]);
    assert_eq!(pool.usable_count(), 2);
}

#[tokio::test]
async fn retries_at_most_three_times() {
    let keys = ["a", "b", "c", "d", "e", "f"];
    let (base, seen) = spawn_keyed_server(
        keys.iter()
            .map(|k| (*k, StatusCode::TOO_MANY_REQUESTS, None))
            .collect(),
    )
    .await;
    let p = OpenAiProvider::new(&base, pool_of(&keys), "m");
    assert!(text_of(&p).await.is_err());
    assert_eq!(
        seen.lock().unwrap().len(),
        4,
        "first try plus three retries"
    );
}

#[tokio::test]
async fn fails_when_every_key_is_unavailable() {
    let (base, seen) = spawn_keyed_server(vec![("a", StatusCode::OK, None)]).await;
    let pool = pool_of(&["a"]);
    pool.mark_invalid(0);
    let p = OpenAiProvider::new(&base, pool, "m");
    assert!(text_of(&p).await.is_err());
    assert!(
        seen.lock().unwrap().is_empty(),
        "no request without a usable key"
    );
}

#[tokio::test]
async fn non_key_client_errors_are_not_retried() {
    let (base, seen) = spawn_keyed_server(vec![
        ("a", StatusCode::BAD_REQUEST, None),
        ("b", StatusCode::OK, None),
    ])
    .await;
    let pool = pool_of(&["a", "b"]);
    let p = OpenAiProvider::new(&base, pool.clone(), "m");
    assert!(text_of(&p).await.is_err());
    assert_eq!(*seen.lock().unwrap(), ["a"]);
    assert_eq!(pool.usable_count(), 2);
}

#[tokio::test]
async fn failure_after_streaming_started_is_not_retried() {
    // 沒有結束標記就斷線：已經開始串流，不能換 key 重送
    let (base, captured) = spawn_server(
        StatusCode::OK,
        "data: {\"choices\":[{\"delta\":{\"content\":\"半\"}}]}\n\n",
    )
    .await;
    let p = OpenAiProvider::new(&base, pool_of(&["a", "b"]), "m");
    assert!(text_of(&p).await.is_err());
    assert_eq!(captured.lock().unwrap().len(), 1);
}
