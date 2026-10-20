//! 全域請求限制（#324）：超過 body 上限一律回 413 與統一的 JSON 錯誤格式（S-08.1）。
//! 全部使用合成資料。

mod common;

use axum::{
    body::{Body, Bytes},
    http::{Request, Response, StatusCode, header},
};
use common::*;
use futures_util::stream;
use serde_json::json;
use socrates_chat_backend::MAX_BODY_BYTES;
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

/// 413 回應必須是統一的 JSON 錯誤格式，且 request_id 與 `x-request-id` 一致。
async fn assert_payload_too_large(res: Response<Body>) {
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let rid = res.headers()["x-request-id"].to_str().unwrap().to_string();
    assert!(
        res.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("application/json")
    );
    let body = json_body(res).await;
    assert_eq!(body["error"]["code"], "payload_too_large");
    assert_eq!(body["error"]["request_id"], rid);
}

#[sqlx::test]
async fn oversized_body_with_content_length_gets_json_413(pool: PgPool) {
    let app = test_app(pool);
    let body = login_body(OVER_LIMIT);
    let req = Request::post("/api/auth/login")
        .header(header::ORIGIN, APP_URL)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CONTENT_LENGTH, body.len())
        .body(Body::from(body))
        .unwrap();
    assert_payload_too_large(send(&app, req).await).await;
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
