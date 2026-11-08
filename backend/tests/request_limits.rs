//! 全域請求限制：超過 body 上限回 413（#324）、一般 API 請求逾時回 408（#325），
//! 兩者都用統一的 JSON 錯誤格式（S-08.1）。全部使用合成資料。

mod common;

use std::time::Duration;

use axum::{
    body::{Body, Bytes},
    http::{Request, Response, StatusCode, header},
};
use common::Script::*;
use common::*;
use futures_util::{StreamExt, stream};
use serde_json::json;
use socrates_chat_backend::{LONG_RUNNING_ROUTES, MAX_BODY_BYTES};
use sqlx::PgPool;

/// 介於全域上限與 axum 預設 2 MB 之間的大小：證明上限是明確設定的，不是沿用預設。
const OVER_LIMIT: usize = MAX_BODY_BYTES + MAX_BODY_BYTES / 2;

fn login_body(pad: usize) -> String {
    json!({"email": "nobody@example.com", "password": "x", "pad": "a".repeat(pad)}).to_string()
}

fn login_req(body: Body) -> Request<Body> {
    Request::post("/api/auth/login")
        .header(header::ORIGIN, APP_URL)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body)
        .unwrap()
}

async fn assert_payload_too_large(res: Response<Body>) {
    assert_json_error(res, StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large").await;
}

#[sqlx::test]
async fn oversized_body_of_known_length_gets_json_413(pool: PgPool) {
    let app = test_app(pool);
    let res = send(&app, login_req(Body::from(login_body(OVER_LIMIT)))).await;
    assert_payload_too_large(res).await;
}

#[sqlx::test]
async fn oversized_chunked_body_gets_json_413(pool: PgPool) {
    let app = test_app(pool);
    // 沒有 Content-Length、分段送來的 body：讀到超過上限時才拒絕
    let chunks: Vec<Result<Bytes, std::io::Error>> = login_body(OVER_LIMIT)
        .into_bytes()
        .chunks(64 * 1024)
        .map(|c| Ok(Bytes::copy_from_slice(c)))
        .collect();
    let res = send(&app, login_req(Body::from_stream(stream::iter(chunks)))).await;
    assert_payload_too_large(res).await;
}

#[sqlx::test]
async fn body_within_the_limit_is_still_handled(pool: PgPool) {
    let app = test_app(pool);
    // 上限內的大 body 照常交給 handler：不存在的帳號回 invalid_credentials
    let res = send(
        &app,
        login_req(Body::from(login_body(MAX_BODY_BYTES - 1024))),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json_body(res).await["error"]["code"], "invalid_credentials");
}

#[sqlx::test]
async fn stalled_body_times_out_with_json_408(pool: PgPool) {
    let app = test_app_with_config(pool, FakeAi::with(vec![]), short_timeout());
    // 送出半個 JSON 後就不再送：模擬慢速（slow-loris）客戶端
    let body = stream::once(async { Ok::<_, std::io::Error>(Bytes::from_static(b"{\"email\":")) })
        .chain(stream::pending());
    // 測試自己的保險：沒有逾時機制時不要永遠卡住
    let res = tokio::time::timeout(
        Duration::from_secs(5),
        send(&app, login_req(Body::from_stream(body))),
    )
    .await
    .expect("server should time out a stalled request body");
    assert_json_error(res, StatusCode::REQUEST_TIMEOUT, "request_timeout").await;
}

#[sqlx::test]
async fn sse_reply_is_not_cut_off_by_the_general_timeout(pool: PgPool) {
    let seed = test_app(pool.clone());
    let a = seed_actors(&seed).await;
    let conv = start_conversation(&seed, &a).await;
    // AI 回覆比一般請求逾時慢得多，串流仍要完整送完
    let ai = FakeAi::with(vec![Paced(vec!["慢慢想"], Duration::from_millis(500))]);
    let app = test_app_with_config(pool, ai, short_timeout());
    let (status, events) = student_says_events(&app, &a, &conv, "我會拉桿").await;
    assert_eq!(status, StatusCode::OK);
    let events = events.unwrap();
    assert!(
        events
            .iter()
            .any(|(n, d)| n == "delta" && d["text"] == "慢慢想")
    );
    assert_eq!(events.last().unwrap().0, "done");
}

#[sqlx::test]
async fn retry_stream_is_not_cut_off_by_the_general_timeout(pool: PgPool) {
    let seed = test_app(pool.clone());
    let a = seed_actors(&seed).await;
    let conv = start_conversation(&seed, &a).await;
    seed_student_message(&pool, &conv, "我會拉桿").await;
    let ai = FakeAi::with(vec![Paced(vec!["慢慢想"], Duration::from_millis(500))]);
    let app = test_app_with_config(pool, ai, short_timeout());
    let res = send(
        &app,
        get(
            &format!("/api/conversations/{conv}/stream"),
            Some(&a.student),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let events = parse_sse(&text_body(res).await);
    assert_eq!(events.last().unwrap().0, "done");
}

#[sqlx::test]
async fn long_timeout_routes_exist(pool: PgPool) {
    // 路由改名時，清單裡的舊路徑會默默退回一般逾時；用 OpenAPI 的路徑確認每一條都還在
    let app = test_app(pool);
    let doc = json_body(send(&app, get("/api/openapi.json", None)).await).await;
    for route in LONG_RUNNING_ROUTES {
        assert!(
            doc["paths"].get(route).is_some(),
            "{route} is not a route anymore"
        );
    }
}
