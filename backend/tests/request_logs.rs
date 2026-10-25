//! 暫時維運日誌頁：只讓管理者查看目前程序記憶體中的安全 request log。

mod common;

use axum::{
    Router,
    http::{StatusCode, header},
};
use common::Script::{Fail, Reply};
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

const SEND_ROUTE: &str = "/api/conversations/{id}/messages";

/// 管理者目前看得到的 request log（最新在前）。
async fn logged(app: &Router, a: &Actors) -> Vec<Value> {
    let body = json_body(send(app, get("/api/admin/logs", Some(&a.admin))).await).await;
    body["items"].as_array().unwrap().clone()
}

/// 用健康檢查填滿整個 ring（500 筆），之後的記錄都會擠掉先前的內容。
async fn fill_ring_with_health_checks(app: &Router) {
    for _ in 0..500 {
        assert_eq!(
            send(app, get("/health", None)).await.status(),
            StatusCode::OK
        );
    }
}

fn entry<'a>(items: &'a [Value], route: &str) -> Option<&'a Value> {
    items.iter().find(|entry| entry["route"] == route)
}

#[sqlx::test]
async fn admin_sees_recent_sanitized_request_logs_newest_first(pool: PgPool) {
    let app = test_app(pool);
    let actors = seed_actors(&app).await;

    let health = send(&app, get("/health", None)).await;
    let health_id = health.headers()[header::HeaderName::from_static("x-request-id")]
        .to_str()
        .unwrap()
        .to_string();
    let missing = send(&app, get("/private/student@example.com", None)).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let missing_id = missing.headers()[header::HeaderName::from_static("x-request-id")]
        .to_str()
        .unwrap()
        .to_string();

    let response = send(&app, get("/api/admin/logs", Some(&actors.admin))).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["capacity"], 500);

    let items = body["items"].as_array().unwrap();
    assert_eq!(items[0]["request_id"], health_id);
    assert_eq!(items[0]["method"], "GET");
    assert_eq!(items[0]["route"], "/health");
    assert_eq!(items[0]["status"], 200);
    assert!(items[0]["error_code"].is_null());
    assert!(items[0]["duration_ms"].is_u64());
    assert!(items[0]["created_at"].is_string());

    // 找不到的路徑不進 ring（見 unknown_path_404s_do_not_evict_recent_history）
    assert!(items.iter().all(|e| e["request_id"] != missing_id));
    assert!(!body.to_string().contains("student@example.com"));
}

#[sqlx::test]
async fn request_logs_are_admin_only(pool: PgPool) {
    let app = test_app(pool);
    let actors = seed_actors(&app).await;

    for (cookie, expected) in [
        (Some(actors.teacher.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.student.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.outsider.as_str()), StatusCode::FORBIDDEN),
        (None, StatusCode::UNAUTHORIZED),
    ] {
        let response = send(&app, get("/api/admin/logs", cookie)).await;
        assert_eq!(response.status(), expected);
    }

    // 被拒絕的維運頁存取不進 ring，因此不會擠掉真正的應用請求
    let items = logged(&app, &actors).await;
    assert!(entry(&items, "/api/admin/logs").is_none());
}

#[sqlx::test]
async fn request_logs_keep_only_the_latest_five_hundred_entries(pool: PgPool) {
    let app = test_app(pool);
    let actors = seed_actors(&app).await;

    for _ in 0..501 {
        assert_eq!(
            send(&app, get("/health", None)).await.status(),
            StatusCode::OK
        );
    }

    let body = json_body(send(&app, get("/api/admin/logs", Some(&actors.admin))).await).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 500);
    assert!(items.iter().all(|entry| entry["route"] == "/health"));
}

#[sqlx::test]
async fn rejected_admin_log_polls_do_not_evict_recent_history(pool: PgPool) {
    let app = test_app(pool);
    let actors = seed_actors(&app).await;
    fill_ring_with_health_checks(&app).await;

    // 過期分頁的輪詢（401）與非管理者的輪詢（403）反覆發生
    for cookie in [
        None,
        Some(actors.teacher.as_str()),
        Some(actors.student.as_str()),
    ] {
        assert!(
            !send(&app, get("/api/admin/logs", cookie))
                .await
                .status()
                .is_success()
        );
    }

    let items = logged(&app, &actors).await;
    assert_eq!(items.len(), 500);
    assert!(items.iter().all(|entry| entry["route"] == "/health"));
}

#[sqlx::test]
async fn unknown_path_404s_do_not_evict_recent_history(pool: PgPool) {
    let app = test_app(pool);
    let actors = seed_actors(&app).await;
    fill_ring_with_health_checks(&app).await;

    for i in 0..5 {
        let res = send(&app, get(&format!("/probe/{i}"), None)).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    let items = logged(&app, &actors).await;
    assert_eq!(items.len(), 500);
    assert!(items.iter().all(|entry| entry["route"] == "/health"));
}

#[sqlx::test]
async fn streamed_reply_is_logged_when_its_stream_ends_with_the_real_outcome(pool: PgPool) {
    // 重試後仍失敗：串流以 error 事件結束，回應標頭本身是 200
    let app = test_app_with_ai(pool, FakeAi::with(vec![Fail, Fail]));
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;

    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/messages"),
            Some(&a.student),
            Some(json!({"content": "我認為應該拉桿。"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    // 標頭已送出但本文尚未讀完：此時不應該寫入 log
    assert!(entry(&logged(&app, &a).await, SEND_ROUTE).is_none());

    let events = parse_sse(&text_body(res).await);
    let (name, data) = events.last().unwrap();
    assert_eq!(name, "error");
    assert_eq!(data["code"], "ai_unavailable");

    let items = logged(&app, &a).await;
    let streamed = entry(&items, SEND_ROUTE).expect("stream is logged once it ends");
    assert_eq!(streamed["method"], "POST");
    assert_eq!(streamed["status"], 200);
    assert_eq!(streamed["error_code"], "ai_unavailable");
}

#[sqlx::test]
async fn successfully_streamed_reply_is_logged_without_an_error_code(pool: PgPool) {
    let app = test_app_with_ai(
        pool,
        FakeAi::with(vec![Reply(vec![
            "你說的「正義」是什麼意思？\n<<<META>>>\n",
            "{\"question_type\":\"clarify\",\"advance\":false}",
        ])]),
    );
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;

    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/messages"),
            Some(&a.student),
            Some(json!({"content": "我認為應該拉桿。"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let events = parse_sse(&text_body(res).await);
    assert_eq!(events.last().unwrap().0, "done");

    let items = logged(&app, &a).await;
    let streamed = entry(&items, SEND_ROUTE).expect("stream is logged once it ends");
    assert_eq!(streamed["status"], 200);
    assert!(streamed["error_code"].is_null());
}
