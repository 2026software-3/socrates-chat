//! 供應商名單與驗證探測（F-40.3，#595）對本機假 HTTP 服務的測試；全部使用合成資料。

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, Method, Response, StatusCode, Uri, header},
};
use serde_json::{Value, json};

use super::*;

const KEY: &str = "sk-test-KEYSENTINEL";
const MODEL: &str = "the-model";
const SHORT: Duration = Duration::from_millis(300);

// ---- 名單 ----

#[test]
fn list_is_pinned_and_excludes_unsafe_or_unsupported_adapters() {
    let ids: Vec<&str> = providers().iter().map(|p| p.id).collect();
    assert_eq!(
        ids,
        [
            "openai",
            "anthropic",
            "gemini",
            "groq",
            "deepseek",
            "xai",
            "mistral",
            "together",
            "fireworks"
        ]
    );
    for excluded in [
        "ollama",
        "ollama_cloud",
        "omlx",
        "vertex",
        "bedrock",
        "github_copilot",
        "opencode_go",
        "openai_resp",
        "custom",
    ] {
        assert!(find(excluded).is_none(), "{excluded}");
    }
    // openai_resp 併入 OpenAI：名單只有 Chat Completions 的 OpenAI adapter
    assert_eq!(find("openai").unwrap().adapter, AdapterKind::OpenAI);
}

#[test]
fn ids_match_the_database_format_and_are_unique() {
    let mut seen = std::collections::HashSet::new();
    for p in providers() {
        assert!(
            !p.id.is_empty()
                && p.id.len() <= 64
                && p.id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-'),
            "{}",
            p.id
        );
        assert!(seen.insert(p.id), "duplicate {}", p.id);
        assert!(!p.name.trim().is_empty());
    }
}

#[test]
fn every_endpoint_is_a_public_https_host_ending_with_a_slash() {
    for p in providers() {
        let url = reqwest::Url::parse(p.endpoint).unwrap_or_else(|_| panic!("{}", p.id));
        assert_eq!(url.scheme(), "https", "{}", p.id);
        assert!(p.endpoint.ends_with('/'), "{}", p.id);
        assert!(
            url.username().is_empty() && url.password().is_none(),
            "{}",
            p.id
        );
        assert!(url.port().is_none(), "{}", p.id);
        assert!(
            url.query().is_none() && url.fragment().is_none(),
            "{}",
            p.id
        );
        let host = url.host_str().unwrap();
        // 一律是網域名稱，不接受 IP 字面值，也不接受本機或內網用的網域
        assert!(host.parse::<std::net::IpAddr>().is_err(), "{host}");
        assert!(host.contains('.'), "{host}");
        for bad in [
            "localhost",
            ".local",
            ".internal",
            ".lan",
            ".home",
            ".localdomain",
        ] {
            assert!(
                !host.ends_with(bad) && host != bad.trim_start_matches('.'),
                "{host}"
            );
        }
    }
}

#[test]
fn find_is_exact_and_case_sensitive() {
    assert_eq!(find("openai").unwrap().id, "openai");
    assert!(find("OpenAI").is_none());
    assert!(find("").is_none());
    assert!(find("openai ").is_none());
}

// ---- 假 HTTP 服務 ----

#[derive(Clone)]
struct Seen {
    method: Method,
    uri: String,
    headers: HeaderMap,
    body: String,
}

type Handler = Arc<dyn Fn(&Seen) -> Response<Body> + Send + Sync>;

#[derive(Clone)]
struct Srv {
    handler: Handler,
    seen: Arc<Mutex<Vec<Seen>>>,
}

async fn handle(
    State(s): State<Srv>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    let seen = Seen {
        method,
        uri: uri.to_string(),
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    };
    s.seen.lock().unwrap().push(seen.clone());
    (s.handler)(&seen)
}

async fn spawn(handler: Handler) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
    let app = Router::new().fallback(handle).with_state(Srv {
        handler,
        seen: seen.clone(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/api/v1/"), seen)
}

fn reply(status: u16, content_type: &str, body: impl Into<String>) -> Response<Body> {
    Response::builder()
        .status(StatusCode::from_u16(status).unwrap())
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body.into()))
        .unwrap()
}

fn fixed(status: u16) -> Handler {
    Arc::new(move |_| reply(status, "application/json", r#"{"data":[]}"#))
}

/// 接受連線但永遠不回應的服務。
async fn hanging() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket);
        }
    });
    format!("http://{addr}/api/v1/")
}

// ---- verify_key ----

const FAMILIES: [AdapterKind; 3] = [
    AdapterKind::OpenAI,
    AdapterKind::Anthropic,
    AdapterKind::Gemini,
];

fn authenticated(adapter: AdapterKind, h: &HeaderMap) -> bool {
    let get = |n: &str| h.get(n).and_then(|v| v.to_str().ok()).map(str::to_owned);
    match adapter {
        AdapterKind::Anthropic => get("x-api-key").as_deref() == Some(KEY),
        AdapterKind::Gemini => get("x-goog-api-key").as_deref() == Some(KEY),
        _ => get("authorization").as_deref() == Some(&format!("Bearer {KEY}")),
    }
}

#[tokio::test]
async fn verify_key_only_lists_models_and_sends_no_conversation_content() {
    for adapter in FAMILIES {
        let (endpoint, seen) = spawn(Arc::new(move |s| {
            if authenticated(adapter, &s.headers) {
                reply(200, "application/json", r#"{"data":[],"models":[]}"#)
            } else {
                reply(401, "application/json", "{}")
            }
        }))
        .await;
        assert_eq!(
            verify_key_at(&endpoint, adapter, KEY, SHORT).await,
            KeyCheck::Usable,
            "{adapter:?}"
        );
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].method, Method::GET);
        assert!(seen[0].uri.starts_with("/api/v1/models"), "{}", seen[0].uri);
        assert!(seen[0].body.is_empty());
        // key 只放在標頭，不放在網址
        assert!(!seen[0].uri.contains(KEY));
    }
}

#[tokio::test]
async fn verify_key_wrong_key_or_rejection_is_connection_failed() {
    for adapter in FAMILIES {
        for status in [400, 401, 403, 404, 500, 503] {
            let (endpoint, _) = spawn(fixed(status)).await;
            assert_eq!(
                verify_key_at(&endpoint, adapter, KEY, SHORT).await,
                KeyCheck::ConnectionFailed,
                "{adapter:?} {status}"
            );
        }
    }
}

#[tokio::test]
async fn verify_key_counts_quota_and_rate_limit_as_usable() {
    for status in [402, 429] {
        let (endpoint, _) = spawn(fixed(status)).await;
        assert_eq!(
            verify_key_at(&endpoint, AdapterKind::OpenAI, KEY, SHORT).await,
            KeyCheck::Usable,
            "{status}"
        );
    }
}

#[tokio::test]
async fn verify_key_unreachable_or_hanging_service_is_connection_failed() {
    // 沒有人在聽的埠
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead = format!("http://{}/v1/", listener.local_addr().unwrap());
    drop(listener);
    assert_eq!(
        verify_key_at(&dead, AdapterKind::OpenAI, KEY, SHORT).await,
        KeyCheck::ConnectionFailed
    );
    // 一直不回應：必須在逾時內回來
    let endpoint = hanging().await;
    let started = std::time::Instant::now();
    assert_eq!(
        verify_key_at(&endpoint, AdapterKind::OpenAI, KEY, SHORT).await,
        KeyCheck::ConnectionFailed
    );
    assert!(started.elapsed() < Duration::from_millis(1200));
}

// ---- verify_model ----

fn sse(data: Value) -> String {
    format!("data: {data}\n\n")
}

fn openai_stream() -> String {
    let mut b = String::new();
    for t in ["O", "K"] {
        b += &sse(
            json!({"id":"c","object":"chat.completion.chunk","model":MODEL,
            "choices":[{"index":0,"delta":{"role":"assistant","content":t}}]}),
        );
    }
    b += &sse(
        json!({"id":"c","object":"chat.completion.chunk","model":MODEL,
        "choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
    );
    b + "data: [DONE]\n\n"
}

fn openai_json(text: &str) -> String {
    json!({"id":"c","object":"chat.completion","model":MODEL,
        "choices":[{"index":0,"message":{"role":"assistant","content":text},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}})
    .to_string()
}

/// 一般的健康供應商：串流與 JSON 結構化輸出都正常。
fn healthy() -> Handler {
    Arc::new(|s| {
        let body: Value = serde_json::from_str(&s.body).unwrap_or_default();
        if body["stream"] == json!(true) {
            reply(200, "text/event-stream", openai_stream())
        } else {
            reply(200, "application/json", openai_json(r#"{"ok":true}"#))
        }
    })
}

#[tokio::test]
async fn verify_model_passes_after_one_stream_and_one_structured_request() {
    let (endpoint, seen) = spawn(healthy()).await;
    assert_eq!(
        verify_model_at(&endpoint, AdapterKind::OpenAI, MODEL, KEY, SHORT).await,
        Ok(())
    );
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    let bodies: Vec<Value> = seen
        .iter()
        .map(|s| serde_json::from_str(&s.body).unwrap())
        .collect();
    assert_eq!(
        bodies.iter().filter(|b| b["stream"] == json!(true)).count(),
        1
    );
    // 結構化輸出請求帶了 JSON schema
    assert!(bodies.iter().any(|b| b.get("response_format").is_some()));
    for s in seen.iter() {
        assert!(s.uri.ends_with("/chat/completions"));
        assert!(authenticated(AdapterKind::OpenAI, &s.headers));
        assert_eq!(
            s.body.matches(MODEL).count(),
            1,
            "model sent once: {}",
            s.body
        );
    }
}

#[tokio::test]
async fn verify_model_reports_a_recognizable_reason_per_failure() {
    use ModelCheckError::*;
    // 認證失敗
    for status in [401, 403] {
        let (e, _) = spawn(fixed(status)).await;
        assert_eq!(
            verify_model_at(&e, AdapterKind::OpenAI, MODEL, KEY, SHORT).await,
            Err(AuthFailed),
            "{status}"
        );
    }
    // 模型不存在
    let (e, _) = spawn(fixed(404)).await;
    assert_eq!(
        verify_model_at(&e, AdapterKind::OpenAI, MODEL, KEY, SHORT).await,
        Err(ModelNotFound)
    );
    // 串流不支援：回的是一般 JSON 而不是事件串流
    let (e, _) = spawn(Arc::new(|_| {
        reply(200, "application/json", openai_json("hi"))
    }))
    .await;
    assert_eq!(
        verify_model_at(&e, AdapterKind::OpenAI, MODEL, KEY, SHORT).await,
        Err(StreamingUnsupported)
    );
    // 結構化輸出不支援：串流正常，但結構化請求被供應商以 400 拒絕
    let (e, _) = spawn(Arc::new(|s| {
        let body: Value = serde_json::from_str(&s.body).unwrap_or_default();
        if body["stream"] == json!(true) {
            reply(200, "text/event-stream", openai_stream())
        } else {
            reply(
                400,
                "application/json",
                r#"{"error":{"message":"response_format unsupported"}}"#,
            )
        }
    }))
    .await;
    assert_eq!(
        verify_model_at(&e, AdapterKind::OpenAI, MODEL, KEY, SHORT).await,
        Err(StructuredOutputUnsupported)
    );
    // 結構化輸出內容不是 JSON 也算不支援
    let (e, _) = spawn(Arc::new(|s| {
        let body: Value = serde_json::from_str(&s.body).unwrap_or_default();
        if body["stream"] == json!(true) {
            reply(200, "text/event-stream", openai_stream())
        } else {
            reply(200, "application/json", openai_json("not json at all"))
        }
    }))
    .await;
    assert_eq!(
        verify_model_at(&e, AdapterKind::OpenAI, MODEL, KEY, SHORT).await,
        Err(StructuredOutputUnsupported)
    );
    // 其他：5xx、限流、連線失敗、逾時
    for status in [429, 500, 503] {
        let (e, _) = spawn(fixed(status)).await;
        assert_eq!(
            verify_model_at(&e, AdapterKind::OpenAI, MODEL, KEY, SHORT).await,
            Err(Other),
            "{status}"
        );
    }
    let e = hanging().await;
    assert_eq!(
        verify_model_at(&e, AdapterKind::OpenAI, MODEL, KEY, SHORT).await,
        Err(Other)
    );
}

#[tokio::test]
async fn reason_codes_are_stable_strings() {
    use ModelCheckError::*;
    let codes: Vec<&str> = [
        AuthFailed,
        ModelNotFound,
        StreamingUnsupported,
        StructuredOutputUnsupported,
        Other,
    ]
    .iter()
    .map(|e| e.code())
    .collect();
    assert_eq!(
        codes,
        [
            "auth_failed",
            "model_not_found",
            "streaming_unsupported",
            "structured_output_unsupported",
            "other"
        ]
    );
}
