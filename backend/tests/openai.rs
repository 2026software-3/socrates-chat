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
async fn reasoning_becomes_one_empty_chunk_before_the_text() {
    // 推理模型先串流推理內容才輸出正文；推理只發出一個空片段（算作首字），推理文字不外流
    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"\",\"reasoning\":\"先想\"}}]}\n\n\
                data: {\"choices\":[{\"delta\":{\"content\":\"\",\"reasoning\":\"再想\"}}]}\n\n\
                data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"還在想\"}}]}\n\n\
                data: {\"choices\":[{\"delta\":{\"content\":\"答案\"}}]}\n\n\
                data: [DONE]\n\n";
    let (base, _) = spawn_server(StatusCode::OK, body).await;
    let p = OpenAiProvider::new(&base, single("k"), "m");
    let chunks: Vec<String> = p
        .stream_chat(request())
        .await
        .unwrap()
        .map(|c| c.unwrap())
        .collect()
        .await;
    assert_eq!(chunks, ["", "答案"]);
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
    pool.mark_rejected(0);
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

#[tokio::test]
async fn waits_for_the_soonest_key_when_all_are_cooling() {
    let (base, seen) = spawn_keyed_server(vec![("a", StatusCode::OK, None)]).await;
    let pool = pool_of(&["a"]);
    pool.mark_rate_limited(0, Some(std::time::Duration::from_millis(250)));
    let p = OpenAiProvider::new(&base, pool, "m");
    let start = std::time::Instant::now();
    assert_eq!(text_of(&p).await.unwrap(), "ok");
    assert!(
        start.elapsed() >= std::time::Duration::from_millis(200),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(*seen.lock().unwrap(), ["a"]);
}

#[tokio::test]
async fn does_not_wait_when_recovery_is_too_far_away() {
    let (base, seen) = spawn_keyed_server(vec![("a", StatusCode::OK, None)]).await;
    let pool = pool_of(&["a"]);
    pool.mark_rate_limited(0, Some(std::time::Duration::from_secs(3600)));
    let p = OpenAiProvider::new(&base, pool, "m");
    let start = std::time::Instant::now();
    assert!(text_of(&p).await.is_err());
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn server_error_with_one_key_is_not_hammered() {
    let (base, seen) = spawn_keyed_server(vec![("a", StatusCode::SERVICE_UNAVAILABLE, None)]).await;
    let p = OpenAiProvider::new(&base, pool_of(&["a"]), "m");
    assert!(text_of(&p).await.is_err());
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn server_error_tries_each_key_at_most_once() {
    let (base, seen) = spawn_keyed_server(vec![
        ("a", StatusCode::BAD_GATEWAY, None),
        ("b", StatusCode::BAD_GATEWAY, None),
    ])
    .await;
    let p = OpenAiProvider::new(&base, pool_of(&["a", "b"]), "m");
    assert!(text_of(&p).await.is_err());
    assert_eq!(*seen.lock().unwrap(), ["a", "b"]);
}

// ---- 整個模型被限流時換備援模型 ----

/// 依請求 body 的 `model` 回不同狀態的假伺服器；記錄每次收到的 (model, key)。
type ModelSeen = Arc<Mutex<Vec<(String, String)>>>;

async fn spawn_model_server(behaviour: Vec<(&'static str, StatusCode)>) -> (String, ModelSeen) {
    spawn_stream_server(
        behaviour
            .into_iter()
            .map(|(m, st)| (m, st, if st.is_success() { OK_BODY } else { "nope" }))
            .collect(),
    )
    .await
}

/// 綁在隨機埠上跑起來，回傳 base URL。
async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn seen_of(seen: &ModelSeen) -> Vec<(String, String)> {
    seen.lock().unwrap().clone()
}

fn pair(model: &str, key: &str) -> (String, String) {
    (model.to_string(), key.to_string())
}

#[tokio::test]
async fn falls_back_to_the_next_model_when_every_key_is_rate_limited() {
    let (base, seen) = spawn_model_server(vec![
        ("primary", StatusCode::TOO_MANY_REQUESTS),
        ("backup", StatusCode::OK),
    ])
    .await;
    let p = OpenAiProvider::new(&base, pool_of(&["a", "b"]), "primary")
        .with_fallbacks(vec!["backup".into()]);
    assert_eq!(text_of(&p).await.unwrap(), "ok");
    // 主模型每把 key 都試過才換；備援用的 key 不會因主模型的 429 而被冷卻
    assert_eq!(
        seen_of(&seen),
        [
            pair("primary", "a"),
            pair("primary", "b"),
            pair("backup", "a")
        ]
    );
}

#[tokio::test]
async fn falls_back_when_the_retry_budget_runs_out_on_rate_limits() {
    // key 比重試次數多：用完重試額度時仍沒有用盡 key，結果還是要算限流而換模型
    let (base, seen) = spawn_model_server(vec![
        ("primary", StatusCode::TOO_MANY_REQUESTS),
        ("backup", StatusCode::OK),
    ])
    .await;
    let p = OpenAiProvider::new(&base, pool_of(&["a", "b", "c", "d", "e"]), "primary")
        .with_fallbacks(vec!["backup".into()]);
    assert_eq!(text_of(&p).await.unwrap(), "ok");
    assert_eq!(
        models_seen(&seen),
        ["primary", "primary", "primary", "primary", "backup"]
    );
}

#[tokio::test]
async fn later_requests_skip_a_cooling_model_without_waiting() {
    let (base, seen) = spawn_model_server(vec![
        ("primary", StatusCode::TOO_MANY_REQUESTS),
        ("backup", StatusCode::OK),
    ])
    .await;
    // 冷卻 5 秒：若還去等主模型的 key 恢復，這個測試會超過時間
    let keys = Arc::new(
        KeyPool::new(vec!["a".into(), "b".into()])
            .unwrap()
            .with_default_cooldown(std::time::Duration::from_secs(5)),
    );
    let p = OpenAiProvider::new(&base, keys, "primary").with_fallbacks(vec!["backup".into()]);
    assert_eq!(text_of(&p).await.unwrap(), "ok");
    let started = std::time::Instant::now();
    assert_eq!(text_of(&p).await.unwrap(), "ok");
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    // 第二次請求沒有再打主模型
    let primary_calls = seen_of(&seen)
        .iter()
        .filter(|(m, _)| m == "primary")
        .count();
    assert_eq!(primary_calls, 2);
}

#[tokio::test]
async fn tries_every_fallback_in_order_and_fails_when_all_are_limited() {
    let (base, seen) = spawn_model_server(vec![
        ("primary", StatusCode::TOO_MANY_REQUESTS),
        ("b1", StatusCode::TOO_MANY_REQUESTS),
        ("b2", StatusCode::TOO_MANY_REQUESTS),
    ])
    .await;
    let p = OpenAiProvider::new(&base, single("k"), "primary")
        .with_fallbacks(vec!["b1".into(), "b2".into()]);
    assert!(text_of(&p).await.is_err());
    assert_eq!(
        seen_of(&seen),
        [pair("primary", "k"), pair("b1", "k"), pair("b2", "k")]
    );
}

#[tokio::test]
async fn skips_a_limited_fallback_and_uses_the_next_one() {
    let (base, seen) = spawn_model_server(vec![
        ("primary", StatusCode::TOO_MANY_REQUESTS),
        ("b1", StatusCode::TOO_MANY_REQUESTS),
        ("b2", StatusCode::OK),
    ])
    .await;
    let p = OpenAiProvider::new(&base, single("k"), "primary")
        .with_fallbacks(vec!["b1".into(), "b2".into()]);
    assert_eq!(text_of(&p).await.unwrap(), "ok");
    assert_eq!(seen_of(&seen).last(), Some(&pair("b2", "k")));
}

#[tokio::test]
async fn does_not_fall_back_when_the_primary_succeeds() {
    let (base, seen) = spawn_model_server(vec![
        ("primary", StatusCode::OK),
        ("backup", StatusCode::OK),
    ])
    .await;
    let p =
        OpenAiProvider::new(&base, single("k"), "primary").with_fallbacks(vec!["backup".into()]);
    assert_eq!(text_of(&p).await.unwrap(), "ok");
    assert_eq!(seen_of(&seen), [pair("primary", "k")]);
}

#[tokio::test]
async fn does_not_fall_back_on_errors_other_than_rate_limiting() {
    for status in [
        StatusCode::BAD_REQUEST,
        StatusCode::UNAUTHORIZED,
        StatusCode::INTERNAL_SERVER_ERROR,
        StatusCode::BAD_GATEWAY,
    ] {
        let (base, seen) =
            spawn_model_server(vec![("primary", status), ("backup", StatusCode::OK)]).await;
        let p = OpenAiProvider::new(&base, pool_of(&["a", "b"]), "primary")
            .with_fallbacks(vec!["backup".into()]);
        assert!(text_of(&p).await.is_err(), "{status}");
        assert!(
            seen_of(&seen).iter().all(|(m, _)| m == "primary"),
            "{status}: 不該打到備援模型"
        );
    }
}

#[tokio::test]
async fn without_fallbacks_a_rate_limited_model_still_fails() {
    let (base, seen) = spawn_model_server(vec![("primary", StatusCode::TOO_MANY_REQUESTS)]).await;
    let p = OpenAiProvider::new(&base, single("k"), "primary");
    assert!(text_of(&p).await.is_err());
    assert_eq!(seen_of(&seen), [pair("primary", "k")]);
}

// ---- 串流內的容量錯誤（HTTP 200，內容是一個 error 區塊）----

const CAPACITY_ERROR: &str = "data: {\"choices\":[],\"error\":{\"code\":502,\"message\":\"Upstream error from Nvidia: ResourceExhausted: Worker local total request limit reached (16/16)\",\"metadata\":{\"error_type\":\"provider_unavailable\"}}}\n\n";
const CODE_429_ERROR: &str =
    "data: {\"choices\":[],\"error\":{\"code\":429,\"message\":\"slow down\"}}\n\n";
const OTHER_ERROR: &str =
    "data: {\"choices\":[],\"error\":{\"code\":502,\"message\":\"Upstream error: Internal\"}}\n\n";
const CONTENT_THEN_CAPACITY_ERROR: &str = "data: {\"choices\":[{\"delta\":{\"content\":\"半\"}}]}\n\n\
    data: {\"choices\":[],\"error\":{\"code\":502,\"message\":\"ResourceExhausted\"}}\n\n";
const REASONING_THEN_CAPACITY_ERROR: &str = "data: {\"choices\":[{\"delta\":{\"reasoning\":\"想\"}}]}\n\n\
    data: {\"choices\":[],\"error\":{\"code\":502,\"message\":\"ResourceExhausted\"}}\n\n";

/// 依 `model` 回 HTTP 200 加指定串流本文的假伺服器（或指定的錯誤狀態）；記錄 (model, key)。
async fn spawn_stream_server(
    behaviour: Vec<(&'static str, StatusCode, &'static str)>,
) -> (String, ModelSeen) {
    #[derive(Clone)]
    struct S {
        by_model: Arc<HashMap<String, (StatusCode, &'static str)>>,
        seen: ModelSeen,
    }
    let seen: ModelSeen = Arc::default();
    let state = S {
        by_model: Arc::new(
            behaviour
                .into_iter()
                .map(|(m, st, body)| (m.to_string(), (st, body)))
                .collect(),
        ),
        seen: seen.clone(),
    };
    let app = Router::new()
        .route(
            "/chat/completions",
            post(
                |State(s): State<S>, h: HeaderMap, Json(v): Json<Value>| async move {
                    let key = h["authorization"]
                        .to_str()
                        .unwrap()
                        .trim_start_matches("Bearer ");
                    let model = v["model"].as_str().unwrap().to_string();
                    s.seen
                        .lock()
                        .unwrap()
                        .push((model.clone(), key.to_string()));
                    let (status, body) = s.by_model[&model];
                    (status, body).into_response()
                },
            ),
        )
        .with_state(state);
    (serve(app).await, seen)
}

fn models_seen(seen: &ModelSeen) -> Vec<String> {
    seen_of(seen).into_iter().map(|(m, _)| m).collect()
}

#[tokio::test]
async fn in_stream_capacity_error_before_any_content_falls_back() {
    for (name, error) in [
        ("ResourceExhausted", CAPACITY_ERROR),
        ("code 429", CODE_429_ERROR),
    ] {
        let (base, seen) = spawn_stream_server(vec![
            ("primary", StatusCode::OK, error),
            ("backup", StatusCode::OK, OK_BODY),
        ])
        .await;
        let p = OpenAiProvider::new(&base, single("k"), "primary")
            .with_fallbacks(vec!["backup".into()]);
        assert_eq!(text_of(&p).await.unwrap(), "ok", "{name}");
        assert_eq!(models_seen(&seen), ["primary", "backup"], "{name}");
    }
}

#[tokio::test]
async fn in_stream_capacity_error_does_not_cool_the_primary_down() {
    // 容量錯誤只是這一次沒搶到名額，不是整個模型暫停：下一個請求仍先試主模型
    let (base, seen) = spawn_stream_server(vec![
        ("primary", StatusCode::OK, CAPACITY_ERROR),
        ("backup", StatusCode::OK, OK_BODY),
    ])
    .await;
    let p = OpenAiProvider::new(&base, pool_of(&["a", "b"]), "primary")
        .with_fallbacks(vec!["backup".into()]);
    assert_eq!(text_of(&p).await.unwrap(), "ok");
    assert_eq!(text_of(&p).await.unwrap(), "ok");
    assert_eq!(
        models_seen(&seen),
        ["primary", "backup", "primary", "backup"]
    );
}

#[tokio::test]
async fn in_stream_capacity_errors_walk_the_whole_chain_in_order() {
    let (base, seen) = spawn_stream_server(vec![
        ("primary", StatusCode::OK, CAPACITY_ERROR),
        ("b1", StatusCode::OK, CAPACITY_ERROR),
        ("b2", StatusCode::OK, OK_BODY),
    ])
    .await;
    let p = OpenAiProvider::new(&base, single("k"), "primary")
        .with_fallbacks(vec!["b1".into(), "b2".into()]);
    assert_eq!(text_of(&p).await.unwrap(), "ok");
    assert_eq!(models_seen(&seen), ["primary", "b1", "b2"]);

    let (base, seen) = spawn_stream_server(vec![
        ("primary", StatusCode::OK, CAPACITY_ERROR),
        ("b1", StatusCode::OK, CAPACITY_ERROR),
    ])
    .await;
    let p = OpenAiProvider::new(&base, single("k"), "primary").with_fallbacks(vec!["b1".into()]);
    assert!(text_of(&p).await.is_err());
    assert_eq!(models_seen(&seen), ["primary", "b1"]);
}

#[tokio::test]
async fn http_429_and_in_stream_capacity_errors_can_be_mixed_in_one_chain() {
    let (base, seen) = spawn_stream_server(vec![
        ("primary", StatusCode::TOO_MANY_REQUESTS, "nope"),
        ("b1", StatusCode::OK, CAPACITY_ERROR),
        ("b2", StatusCode::OK, OK_BODY),
    ])
    .await;
    let p = OpenAiProvider::new(&base, single("k"), "primary")
        .with_fallbacks(vec!["b1".into(), "b2".into()]);
    assert_eq!(text_of(&p).await.unwrap(), "ok");
    assert_eq!(models_seen(&seen), ["primary", "b1", "b2"]);
}

#[tokio::test]
async fn other_in_stream_errors_do_not_fall_back() {
    let (base, seen) = spawn_stream_server(vec![
        ("primary", StatusCode::OK, OTHER_ERROR),
        ("backup", StatusCode::OK, OK_BODY),
    ])
    .await;
    let p =
        OpenAiProvider::new(&base, single("k"), "primary").with_fallbacks(vec!["backup".into()]);
    assert!(text_of(&p).await.is_err());
    assert_eq!(models_seen(&seen), ["primary"]);
}

#[tokio::test]
async fn capacity_error_after_content_or_reasoning_started_does_not_fall_back() {
    // 已經有內容（或推理，呼叫端已據此解除首字逾時）給了學生，不能再換成另一個模型的回覆
    for body in [CONTENT_THEN_CAPACITY_ERROR, REASONING_THEN_CAPACITY_ERROR] {
        let (base, seen) = spawn_stream_server(vec![
            ("primary", StatusCode::OK, body),
            ("backup", StatusCode::OK, OK_BODY),
        ])
        .await;
        let p = OpenAiProvider::new(&base, single("k"), "primary")
            .with_fallbacks(vec!["backup".into()]);
        assert!(text_of(&p).await.is_err());
        assert_eq!(models_seen(&seen), ["primary"]);
    }
}

#[tokio::test]
async fn without_fallbacks_an_in_stream_capacity_error_still_fails() {
    let (base, seen) = spawn_stream_server(vec![("primary", StatusCode::OK, CAPACITY_ERROR)]).await;
    let p = OpenAiProvider::new(&base, single("k"), "primary");
    assert!(text_of(&p).await.is_err());
    assert_eq!(models_seen(&seen), ["primary"]);
}

#[tokio::test]
async fn with_fallbacks_ignores_the_primary_and_duplicates() {
    let (base, seen) = spawn_model_server(vec![
        ("primary", StatusCode::TOO_MANY_REQUESTS),
        ("b1", StatusCode::TOO_MANY_REQUESTS),
    ])
    .await;
    let p = OpenAiProvider::new(&base, single("k"), "primary").with_fallbacks(vec![
        "primary".into(),
        "b1".into(),
        "b1".into(),
    ]);
    assert!(text_of(&p).await.is_err());
    assert_eq!(models_seen(&seen), ["primary", "b1"]);
}
