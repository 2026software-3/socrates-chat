//! `GenaiProvider` 對本機假 HTTP 服務的測試（#594）：只從外面觀察它送出什麼、吐出什麼，
//! 涵蓋 OpenAI 相容、Anthropic、Gemini 三種協定家族；不呼叫真的供應商。
//! 全部使用合成資料。

use std::{
    io,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, Response, StatusCode, Uri, header},
};
use futures_util::{StreamExt, stream};
use serde_json::{Value, json};
use tracing_subscriber::fmt::MakeWriter;

use super::*;
use crate::{
    ai::{ChatRole, ChatTurn},
    diagnostics::Failure,
    logging,
};

const MODEL: &str = "the-model";
const KEY: &str = "sk-test-KEYSENTINEL";
const SYSTEM_TEXT: &str = "SYSTEMPROMPT-SENTINEL 你是蘇格拉底";
const STUDENT_TEXT: &str = "STUDENTMSG-SENTINEL 我覺得存在主義很難懂";
const PROVIDER_BODY: &str = "PROVIDERBODY-SENTINEL";
const SENTINELS: [&str; 4] = [KEY, SYSTEM_TEXT, STUDENT_TEXT, PROVIDER_BODY];
const JSON_TEXT: &str = r#"{"zz_marker_field":"ok"}"#;

#[derive(Clone, Copy, Debug)]
enum Family {
    OpenAi,
    Anthropic,
    Gemini,
}

const FAMILIES: [Family; 3] = [Family::OpenAi, Family::Anthropic, Family::Gemini];

fn sse(event: Option<&str>, data: Value) -> String {
    match event {
        Some(e) => format!("event: {e}\ndata: {data}\n\n"),
        None => format!("data: {data}\n\n"),
    }
}

impl Family {
    fn adapter(self) -> AdapterKind {
        match self {
            Self::OpenAi => AdapterKind::OpenAI,
            Self::Anthropic => AdapterKind::Anthropic,
            Self::Gemini => AdapterKind::Gemini,
        }
    }

    /// 各供應商文件上的認證方式（與 genai 的實作無關）。
    fn authenticated(self, h: &HeaderMap) -> bool {
        let get = |name: &str| h.get(name).and_then(|v| v.to_str().ok()).map(str::to_owned);
        match self {
            Self::OpenAi => get("authorization").is_some_and(|v| v == format!("Bearer {KEY}")),
            Self::Anthropic => get("x-api-key").as_deref() == Some(KEY),
            Self::Gemini => get("x-goog-api-key").as_deref() == Some(KEY),
        }
    }

    /// 串流回應：「你」「好」兩段文字；`usage` 為真時帶 11 輸入、7 輸出 token。
    fn stream_body(self, usage: bool) -> String {
        match self {
            Self::OpenAi => {
                let mut b = String::new();
                for t in ["你", "好"] {
                    b += &sse(
                        None,
                        json!({"id":"c","object":"chat.completion.chunk","model":MODEL,
                            "choices":[{"index":0,"delta":{"role":"assistant","content":t}}]}),
                    );
                }
                b += &sse(
                    None,
                    json!({"id":"c","object":"chat.completion.chunk","model":MODEL,
                        "choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
                );
                if usage {
                    b += &sse(
                        None,
                        json!({"id":"c","object":"chat.completion.chunk","model":MODEL,"choices":[],
                            "usage":{"prompt_tokens":11,"completion_tokens":7,"total_tokens":18}}),
                    );
                }
                b + "data: [DONE]\n\n"
            }
            Self::Anthropic => {
                let start_usage = if usage {
                    json!({"input_tokens":11,"output_tokens":1})
                } else {
                    json!({})
                };
                let mut b = sse(
                    Some("message_start"),
                    json!({"type":"message_start","message":{"id":"m","type":"message",
                        "role":"assistant","model":MODEL,"content":[],"usage":start_usage}}),
                );
                b += &sse(
                    Some("content_block_start"),
                    json!({"type":"content_block_start","index":0,
                        "content_block":{"type":"text","text":""}}),
                );
                for t in ["你", "好"] {
                    b += &sse(
                        Some("content_block_delta"),
                        json!({"type":"content_block_delta","index":0,
                            "delta":{"type":"text_delta","text":t}}),
                    );
                }
                b += &sse(
                    Some("content_block_stop"),
                    json!({"type":"content_block_stop","index":0}),
                );
                let delta_usage = if usage {
                    json!({"output_tokens":7})
                } else {
                    json!({})
                };
                b += &sse(
                    Some("message_delta"),
                    json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},
                        "usage":delta_usage}),
                );
                b + &sse(Some("message_stop"), json!({"type":"message_stop"}))
            }
            Self::Gemini => {
                let mut b = sse(
                    None,
                    json!({"candidates":[{"content":{"role":"model","parts":[{"text":"你"}]}}]}),
                );
                let mut last = json!({"candidates":[{"content":{"role":"model",
                    "parts":[{"text":"好"}]},"finishReason":"STOP"}]});
                if usage {
                    last["usageMetadata"] = json!({"promptTokenCount":11,
                        "candidatesTokenCount":7,"totalTokenCount":18});
                }
                b += &sse(None, last);
                b
            }
        }
    }

    /// 非串流回應（結構化輸出），一律帶 11 輸入、7 輸出 token。
    fn json_body(self, text: &str) -> String {
        match self {
            Self::OpenAi => json!({"id":"c","object":"chat.completion","model":MODEL,
                "choices":[{"index":0,"message":{"role":"assistant","content":text},
                    "finish_reason":"stop"}],
                "usage":{"prompt_tokens":11,"completion_tokens":7,"total_tokens":18}}),
            Self::Anthropic => json!({"id":"m","type":"message","role":"assistant","model":MODEL,
                "content":[{"type":"text","text":text}],"stop_reason":"end_turn",
                "usage":{"input_tokens":11,"output_tokens":7}}),
            Self::Gemini => json!({"candidates":[{"content":{"role":"model",
                "parts":[{"text":text}]},"finishReason":"STOP"}],
                "usageMetadata":{"promptTokenCount":11,"candidatesTokenCount":7,
                    "totalTokenCount":18},"modelVersion":MODEL}),
        }
        .to_string()
    }
}

// ---- 假 HTTP 服務 ----

#[derive(Clone)]
enum Reply {
    Body {
        status: u16,
        content_type: &'static str,
        body: String,
        headers: Vec<(&'static str, &'static str)>,
    },
    /// 先送出一段串流內容，再把連線異常中斷。
    StreamThenAbort(String),
}

impl Reply {
    fn stream(body: String) -> Self {
        Self::Body {
            status: 200,
            content_type: "text/event-stream",
            body,
            headers: vec![],
        }
    }
    fn json(body: String) -> Self {
        Self::Body {
            status: 200,
            content_type: "application/json",
            body,
            headers: vec![],
        }
    }
    fn failure(status: u16, headers: Vec<(&'static str, &'static str)>, body: String) -> Self {
        Self::Body {
            status,
            content_type: "application/json",
            body,
            headers,
        }
    }
}

#[derive(Debug, Clone)]
struct Seen {
    uri: String,
    headers: HeaderMap,
    body: String,
}

type Requests = Arc<Mutex<Vec<Seen>>>;

#[derive(Clone)]
struct Srv {
    reply: Reply,
    seen: Requests,
}

async fn handle(State(s): State<Srv>, uri: Uri, headers: HeaderMap, body: Bytes) -> Response<Body> {
    s.seen.lock().unwrap().push(Seen {
        uri: uri.to_string(),
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    });
    match s.reply {
        Reply::Body {
            status,
            content_type,
            body,
            headers,
        } => {
            let mut b = Response::builder()
                .status(StatusCode::from_u16(status).unwrap())
                .header(header::CONTENT_TYPE, content_type);
            for (k, v) in headers {
                b = b.header(k, v);
            }
            b.body(Body::from(body)).unwrap()
        }
        Reply::StreamThenAbort(first) => {
            // 先讓第一段送達再斷線；馬上斷會讓 TCP RST 把客戶端還沒讀的資料丟掉
            let body = stream::once(async move { Ok::<_, io::Error>(Bytes::from(first)) }).chain(
                stream::once(async {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Err(io::Error::other("connection reset"))
                }),
            );
            Response::builder()
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from_stream(body))
                .unwrap()
        }
    }
}

/// 回傳（端點，收到的請求）。端點以 `/` 結尾，與供應商名單提供的固定端點同形。
async fn spawn(reply: Reply) -> (String, Requests) {
    let seen: Requests = Arc::default();
    let app = Router::new().fallback(handle).with_state(Srv {
        reply,
        seen: seen.clone(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/api/v1/"), seen)
}

fn provider(family: Family, endpoint: &str) -> GenaiProvider {
    GenaiClient::new()
        .unwrap()
        .provider(AiTarget::new(family.adapter(), endpoint, MODEL, KEY))
}

fn chat_request() -> AiRequest {
    AiRequest {
        system: SYSTEM_TEXT.into(),
        messages: vec![ChatTurn {
            role: ChatRole::User,
            content: STUDENT_TEXT.into(),
        }],
    }
}

fn json_request() -> StructuredRequest {
    StructuredRequest {
        system: SYSTEM_TEXT.into(),
        input: json!({"student": STUDENT_TEXT}),
        schema: json!({"type":"object",
            "properties":{"zz_marker_field":{"type":"string"}},
            "required":["zz_marker_field"],"additionalProperties":false}),
        name: "test_schema".into(),
    }
}

/// 串流到底；回傳已收到的文字與最後的錯誤（若有）。
async fn drain(provider: &GenaiProvider) -> (String, Option<AiError>) {
    let mut stream = match provider.stream_chat(chat_request()).await {
        Ok(s) => s,
        Err(e) => return (String::new(), Some(e)),
    };
    let mut text = String::new();
    while let Some(item) = stream.next().await {
        match item {
            Ok(t) => text.push_str(&t),
            Err(e) => return (text, Some(e)),
        }
    }
    (text, None)
}

fn assert_clean(error: &AiError, context: &str) {
    let shown = format!("{error:?}");
    for s in SENTINELS {
        assert!(!shown.contains(s), "{context}: error leaks {s}: {shown}");
    }
}

// ---- 串流對話 ----

#[tokio::test]
async fn streams_text_with_the_explicit_key_model_and_messages_for_every_family() {
    for family in FAMILIES {
        let (endpoint, seen) = spawn(Reply::stream(family.stream_body(false))).await;
        let (text, error) = drain(&provider(family, &endpoint)).await;
        assert!(error.is_none(), "{family:?}: {error:?}");
        assert_eq!(text, "你好", "{family:?}");

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "{family:?}: exactly one request");
        let req = &seen[0];
        assert!(family.authenticated(&req.headers), "{family:?}: {req:?}");
        assert!(req.body.contains(SYSTEM_TEXT), "{family:?}: system prompt");
        assert!(req.body.contains(STUDENT_TEXT), "{family:?}: student turn");
        assert!(
            req.uri.contains(MODEL) || req.body.contains(MODEL),
            "{family:?}: model"
        );
        assert!(
            !req.uri.contains(KEY) && !req.body.contains(KEY),
            "{family:?}: key only in the auth header"
        );
        for advanced in ["reasoning", "temperature", "thinking"] {
            assert!(
                !req.body.contains(advanced),
                "{family:?}: sends no advanced param {advanced}"
            );
        }
    }
}

#[tokio::test]
async fn reports_the_provider_token_usage_after_streaming() {
    for family in FAMILIES {
        let (endpoint, _) = spawn(Reply::stream(family.stream_body(true))).await;
        let p = provider(family, &endpoint);
        let (_, error) = drain(&p).await;
        assert!(error.is_none(), "{family:?}: {error:?}");
        assert_eq!(
            p.last_usage(),
            TokenUsage {
                input: Some(11),
                output: Some(7)
            },
            "{family:?}"
        );
    }
}

#[tokio::test]
async fn usage_stays_unreported_when_the_provider_gives_none() {
    for family in FAMILIES {
        let (endpoint, _) = spawn(Reply::stream(family.stream_body(false))).await;
        let p = provider(family, &endpoint);
        let (text, error) = drain(&p).await;
        assert!(error.is_none() && text == "你好", "{family:?}: {error:?}");
        assert_eq!(p.last_usage(), TokenUsage::default(), "{family:?}");
    }
}

// ---- 結構化輸出 ----

#[tokio::test]
async fn complete_json_sends_the_schema_and_returns_text_model_and_usage() {
    for family in FAMILIES {
        let (endpoint, seen) = spawn(Reply::json(family.json_body(JSON_TEXT))).await;
        let p = provider(family, &endpoint);
        let done = p
            .complete_json(json_request())
            .await
            .unwrap_or_else(|e| panic!("{family:?}: {e:?}"));
        assert_eq!(done.content, JSON_TEXT, "{family:?}");
        assert_eq!(done.model, MODEL, "{family:?}");
        assert_eq!(
            p.last_usage(),
            TokenUsage {
                input: Some(11),
                output: Some(7)
            },
            "{family:?}"
        );

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "{family:?}");
        let req = &seen[0];
        assert!(family.authenticated(&req.headers), "{family:?}");
        assert!(req.body.contains("zz_marker_field"), "{family:?}: schema");
        assert!(req.body.contains(SYSTEM_TEXT), "{family:?}: system");
        assert!(req.body.contains(STUDENT_TEXT), "{family:?}: input");
    }
}

#[tokio::test]
async fn a_successful_reply_without_usable_text_is_invalid_output() {
    for family in FAMILIES {
        let body = json!({"unexpected": PROVIDER_BODY}).to_string();
        let (endpoint, _) = spawn(Reply::json(body)).await;
        let error = provider(family, &endpoint)
            .complete_json(json_request())
            .await
            .expect_err("no text must not be a success");
        assert_eq!(error.class, Failure::InvalidOutput, "{family:?}");
        assert_clean(&error, &format!("{family:?}"));
    }
}

// ---- 失敗分類 ----

struct FailureCase {
    status: u16,
    headers: Vec<(&'static str, &'static str)>,
    body: String,
    class: Failure,
    retry_secs: Option<u64>,
}

fn case(
    status: u16,
    headers: Vec<(&'static str, &'static str)>,
    body: &str,
    class: Failure,
    retry_secs: Option<u64>,
) -> FailureCase {
    FailureCase {
        status,
        headers,
        body: body.to_owned(),
        class,
        retry_secs,
    }
}

fn failure_table() -> Vec<FailureCase> {
    let plain = json!({"error": {"message": PROVIDER_BODY}}).to_string();
    let quota =
        json!({"error": {"code": "insufficient_quota", "message": PROVIDER_BODY}}).to_string();
    vec![
        case(401, vec![], &plain, Failure::InvalidKey, None),
        case(403, vec![], &plain, Failure::InvalidKey, None),
        case(402, vec![], &plain, Failure::QuotaExceeded, None),
        case(
            429,
            vec![("retry-after", "7")],
            &plain,
            Failure::RateLimited,
            Some(7),
        ),
        case(429, vec![], &plain, Failure::RateLimited, None),
        case(429, vec![], &quota, Failure::QuotaExceeded, None),
        case(500, vec![], &plain, Failure::ServiceError, None),
        case(503, vec![], &plain, Failure::ServiceError, None),
    ]
}

#[tokio::test]
async fn classifies_http_failures_before_any_text_for_both_calls() {
    for family in FAMILIES {
        for FailureCase {
            status,
            headers,
            body,
            class,
            retry_secs,
        } in failure_table()
        {
            let (endpoint, _) = spawn(Reply::failure(status, headers, body)).await;
            let p = provider(family, &endpoint);
            let want_retry = retry_secs.map(Duration::from_secs);

            let (text, error) = drain(&p).await;
            let error = error.unwrap_or_else(|| panic!("{family:?} {status}: stream succeeded"));
            assert!(
                text.is_empty(),
                "{family:?} {status}: failed before any text"
            );
            assert_eq!(error.class, class, "{family:?} {status} stream");
            assert_eq!(
                error.retry_after(),
                want_retry,
                "{family:?} {status} stream"
            );
            assert_clean(&error, &format!("{family:?} {status} stream"));

            let error = p
                .complete_json(json_request())
                .await
                .expect_err("failure must not be a success");
            assert_eq!(error.class, class, "{family:?} {status} json");
            assert_eq!(error.retry_after(), want_retry, "{family:?} {status} json");
            assert_clean(&error, &format!("{family:?} {status} json"));
        }
    }
}

#[tokio::test]
async fn an_unreachable_endpoint_is_a_connection_failure() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/api/v1/", listener.local_addr().unwrap());
    drop(listener);
    for family in FAMILIES {
        let (_, error) = drain(&provider(family, &endpoint)).await;
        let error = error.unwrap_or_else(|| panic!("{family:?}: connected to nothing"));
        assert_eq!(error.class, Failure::ConnectionFailed, "{family:?}");
        assert_clean(&error, &format!("{family:?}"));
    }
}

#[tokio::test]
async fn a_connection_cut_mid_stream_is_an_error_after_the_text_already_sent() {
    for family in FAMILIES {
        // 只取第一段文字之前的內容，後面直接斷線
        let first = family
            .stream_body(false)
            .split("\n\n")
            .take(match family {
                Family::OpenAi => 1,
                Family::Anthropic => 3,
                Family::Gemini => 1,
            })
            .collect::<Vec<_>>()
            .join("\n\n")
            + "\n\n";
        let (endpoint, _) = spawn(Reply::StreamThenAbort(first)).await;
        let (text, error) = drain(&provider(family, &endpoint)).await;
        assert_eq!(text, "你", "{family:?}: text before the cut is delivered");
        let error = error.unwrap_or_else(|| panic!("{family:?}: a cut stream looked complete"));
        assert_ne!(error.class, Failure::InvalidOutput, "{family:?}");
        assert_clean(&error, &format!("{family:?}"));
    }
}

// ---- 日誌不洩漏 ----

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

struct Sink(Arc<Mutex<Vec<u8>>>);

impl io::Write for Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Sink;
    fn make_writer(&'a self) -> Sink {
        Sink(self.0.clone())
    }
}

/// 走過各種失敗路徑（HTTP 錯誤、無法解析的成功回應、串流中斷），以實際會用的 filter
/// （就算 `RUST_LOG=trace`）擷取全部日誌，不得出現 key、系統提示、學生訊息或供應商回應本文。
#[tokio::test]
async fn logs_never_contain_keys_prompts_conversation_or_provider_bodies() {
    let captured = Captured::default();
    let subscriber = logging::json_subscriber()
        .with_env_filter(logging::env_filter(Some("trace")))
        .with_writer(captured.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    for family in FAMILIES {
        for c in failure_table() {
            let (endpoint, _) = spawn(Reply::failure(c.status, c.headers, c.body)).await;
            let p = provider(family, &endpoint);
            let _ = drain(&p).await;
            let _ = p.complete_json(json_request()).await;
        }
        let (endpoint, _) = spawn(Reply::json(
            json!({"unexpected": PROVIDER_BODY, "echo": STUDENT_TEXT}).to_string(),
        ))
        .await;
        let _ = provider(family, &endpoint)
            .complete_json(json_request())
            .await;

        let (endpoint, _) = spawn(Reply::StreamThenAbort(format!(
            "data: {}\n\n",
            json!({"echo": STUDENT_TEXT, "body": PROVIDER_BODY})
        )))
        .await;
        let _ = drain(&provider(family, &endpoint)).await;
    }

    let logs = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    for s in SENTINELS {
        assert!(!logs.contains(s), "logs leak {s}");
    }
}
