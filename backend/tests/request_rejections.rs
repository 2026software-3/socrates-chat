//! 請求被 extractor 或路由層拒絕時的回應格式，以及已移除的 request log 介面。

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use common::*;
use socrates_chat_backend::openapi;
use sqlx::PgPool;

/// 程序記憶體的 request log 已由持久化診斷取代（#465、S-09.3）：路由不存在，也不在 API 契約裡。
#[sqlx::test]
async fn the_in_memory_request_log_endpoint_is_gone(pool: PgPool) {
    let app = test_app(pool);
    let actors = seed_actors(&app).await;

    for cookie in [Some(actors.admin.as_str()), None] {
        let res = send(&app, get("/api/admin/logs", cookie)).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        assert_eq!(json_body(res).await["error"]["code"], "not_found");
    }

    let document = serde_json::to_value(openapi::document()).unwrap();
    assert!(document["paths"]["/api/admin/logs"].is_null());
}

/// 帶任意本文與 content-type 的 POST（繞過 `json_req`，才能送出壞掉的請求）。
fn raw_post(path: &str, cookie: &str, content_type: Option<&str>, body: &str) -> Request<Body> {
    let mut b = Request::post(path)
        .header(header::ORIGIN, APP_URL)
        .header(header::COOKIE, cookie);
    if let Some(ct) = content_type {
        b = b.header(header::CONTENT_TYPE, ct);
    }
    b.body(Body::from(body.to_string())).unwrap()
}

#[sqlx::test]
async fn json_extractor_rejections_use_the_api_error_format(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;

    for (content_type, body, status, code) in [
        (
            Some("application/json"),
            "{not json",
            StatusCode::BAD_REQUEST,
            "bad_request",
        ),
        (
            None,
            "{}",
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
        ),
        (
            Some("application/json"),
            "{}",
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_body",
        ),
    ] {
        let res = send(
            &app,
            raw_post("/api/topics", &a.teacher, content_type, body),
        )
        .await;
        assert_eq!(res.status(), status, "{body}");
        let json = json_body(res).await;
        assert_eq!(json["error"]["code"], code);
        assert!(
            json["error"]["request_id"]
                .as_str()
                .is_some_and(|id| !id.is_empty())
        );
    }
}

#[sqlx::test]
async fn rejections_outside_the_api_are_left_alone(pool: PgPool) {
    let app = test_app(pool);
    let res = send(
        &app,
        raw_post("/health", "sid=none", Some("application/json"), "{}"),
    )
    .await;
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert!(res.headers().contains_key(header::ALLOW));
    assert!(!json_body(res).await["error"]["code"].is_string());
}
