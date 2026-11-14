mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test]
async fn status_requires_admin_and_reports_no_ai_data_without_a_probe(pool: PgPool) {
    let ai = FakeAi::with(vec![]);
    let app = test_app_with_ai(pool, ai.clone());
    let actors = seed_actors(&app).await;
    for (cookie, status) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(actors.student.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.teacher.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.outsider.as_str()), StatusCode::FORBIDDEN),
    ] {
        for path in ["/api/admin/status", "/api/admin/diagnostics"] {
            assert_eq!(send(&app, get(path, cookie)).await.status(), status);
        }
    }
    let response = send(&app, get("/api/admin/status", Some(&actors.admin))).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["service"], "ok");
    assert_eq!(body["database"], "ok");
    assert_eq!(body["statistics"]["ai_status"], "no_data");
    assert_eq!(ai.request_count(), 0);
}

#[sqlx::test]
async fn unknown_login_failure_is_safe_and_queryable_after_recreating_the_app(pool: PgPool) {
    let app = test_app(pool.clone());
    let actors = seed_actors(&app).await;
    let response = send(&app, json_req(
        "POST", "/api/auth/login", None,
        Some(json!({"account": "missing-diagnostic@example.com", "password": "synthetic-secret"})),
    )).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    drop(app);
    socrates_chat_backend::diagnostics::flush().await;
    let restarted = test_app(pool);
    let response = send(
        &restarted,
        get(
            &format!("/api/admin/diagnostics?request_id={id}"),
            Some(&actors.admin),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let event = &body["items"][0];
    assert_eq!(event["request_id"], id);
    assert_eq!(event["failure_class"], "invalid_credentials");
    assert!(event["user_id"].is_null());
    let serialized = body.to_string();
    assert!(!serialized.contains("missing-diagnostic"));
    assert!(!serialized.contains("synthetic-secret"));
}

async fn admin_json(app: &axum::Router, cookie: &str, path: &str) -> Value {
    // 診斷事件在背景寫入，讀取前先等進行中的寫入完成
    socrates_chat_backend::diagnostics::flush().await;
    let response = send(app, get(path, Some(cookie))).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

#[sqlx::test]
async fn windows_filter_errors_hide_successes_and_purge_expired_events(pool: PgPool) {
    let app = test_app(pool.clone());
    let actors = seed_actors(&app).await;
    let request_id = Uuid::new_v4();
    let classroom = Uuid::new_v4();
    let user = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO diagnostic_events
            (occurred_at, request_id, kind, failure_class, function, route, status, error_code,
             user_id, classroom_id, duration_ms, retry_count, model)
         VALUES
            (now() - interval '2 hours', $1, 'ai', NULL, 'chat_reply', '/api/conversations/{id}/messages',
             200, NULL, $2, $3, 10, 0, 'synthetic-model'),
            (now() - interval '3 days', $1, 'ai', 'timeout', 'chat_reply', '/api/conversations/{id}/messages',
             200, 'ai_unavailable', $2, $3, 15, 1, 'synthetic-model'),
            (now() - interval '10 days', $1, 'login', 'invalid_credentials', 'password_login', '/api/auth/login',
             401, 'invalid_credentials', $2, NULL, NULL, NULL, NULL),
            (now() - interval '10 days', $1, 'account_lock', 'account_locked', 'password_login', '/api/auth/login',
             401, 'invalid_credentials', $2, NULL, NULL, NULL, NULL),
            (now() - interval '31 days', $1, 'ai', 'service_error', 'chat_reply', '/api/conversations/{id}/messages',
             200, 'ai_unavailable', $2, $3, 12, 0, 'synthetic-model')",
    ).bind(request_id).bind(user).bind(classroom).execute(&pool).await.unwrap();

    let daily = admin_json(&app, &actors.admin, "/api/admin/status").await;
    assert_eq!(
        daily["statistics"],
        json!({
            "ai_status": "ok", "ai_calls": 1, "ai_failures": 0, "ai_invalid_outputs": 0,
            "login_failures": 0, "account_locks": 0,
        })
    );
    assert_eq!(daily["recent_errors"], json!([]));
    let weekly = admin_json(&app, &actors.admin, "/api/admin/status?period=7d").await;
    assert_eq!(weekly["statistics"]["ai_status"], "has_failures");
    assert_eq!(weekly["statistics"]["ai_failures"], 1);
    assert_eq!(weekly["statistics"]["account_locks"], 0);
    let monthly = admin_json(&app, &actors.admin, "/api/admin/status?period=30d").await;
    assert_eq!(monthly["statistics"]["ai_failures"], 1);
    assert_eq!(monthly["statistics"]["login_failures"], 1);
    assert_eq!(monthly["statistics"]["account_locks"], 1);
    let filter = format!(
        "/api/admin/diagnostics?request_id={request_id}&classroom_id={classroom}&user_id={user}&kind=ai&failure_class=timeout",
    );
    let filtered = admin_json(&app, &actors.admin, &filter).await;
    assert_eq!(filtered["items"].as_array().unwrap().len(), 1);
    assert_eq!(filtered["items"][0]["retry_count"], 1);
    let path = "/api/admin/diagnostics?limit=1";
    let page = admin_json(&app, &actors.admin, path).await;
    let cursor = page["next_cursor"].as_str().unwrap();
    let next = admin_json(&app, &actors.admin, &format!("{path}&cursor={cursor}")).await;
    assert_ne!(page["items"], next["items"]);
    assert_eq!(
        send(
            &app,
            get(
                &format!("{path}&kind=login&cursor={cursor}"),
                Some(&actors.admin)
            )
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let end = (chrono::Utc::now() - chrono::Duration::days(7)).format("%Y-%m-%dT%H:%M:%SZ");
    let older = admin_json(
        &app,
        &actors.admin,
        &format!("/api/admin/diagnostics?to={end}"),
    )
    .await;
    assert_eq!(older["items"].as_array().unwrap().len(), 2);

    assert_eq!(
        socrates_chat_backend::diagnostics::purge_expired(&pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        socrates_chat_backend::diagnostics::purge_expired(&pool)
            .await
            .unwrap(),
        0
    );
    sqlx::migrate!().run(&pool).await.unwrap();
}

#[sqlx::test]
async fn background_reply_failures_keep_the_initiating_request_and_classroom(pool: PgPool) {
    let ai = FakeAi::with(vec![Script::Fail, Script::Fail]);
    let app = test_app_with_ai(pool, ai);
    let actors = seed_actors(&app).await;
    let conversation = start_conversation(&app, &actors).await;
    let response = send(
        &app,
        json_req(
            "POST",
            &format!("/api/conversations/{conversation}/messages"),
            Some(&actors.student),
            Some(json!({"content": "synthetic chat text never stored in diagnostics"})),
        ),
    )
    .await;
    let id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(text_body(response).await.contains("ai_unavailable"));
    let filter = format!(
        "/api/admin/diagnostics?request_id={id}&classroom_id={}",
        actors.classroom
    );
    let body = admin_json(&app, &actors.admin, &filter).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    // 兩次嘗試各自背景寫入，不依賴寫入順序
    let mut retries: Vec<i64> = items
        .iter()
        .map(|item| item["retry_count"].as_i64().unwrap())
        .collect();
    retries.sort_unstable();
    assert_eq!(retries, [0, 1]);
    for item in items {
        assert_eq!(item["function"], "chat_reply");
        assert_eq!(item["route"], "/api/conversations/{id}/messages");
        assert_eq!(item["failure_class"], "service_error");
        assert!(item["user_id"].as_str().is_some());
        assert_eq!(item["model"], "test-model");
    }
    assert!(!body.to_string().contains("synthetic chat text"));
}

#[sqlx::test]
async fn attempts_while_locked_and_ip_only_locks_do_not_add_events(pool: PgPool) {
    let app = test_app(pool);
    let actors = seed_actors(&app).await;
    let max = socrates_chat_backend::auth_store::MAX_LOGIN_FAILURES;
    for attempt in 0..=max {
        let response = send(&app, json_req("POST", "/api/auth/login", None,
            Some(json!({"account": "missing-lock@example.com", "password": "wrong-synthetic-password"})),
        )).await;
        assert_eq!(
            response.status(),
            if attempt == max {
                StatusCode::TOO_MANY_REQUESTS
            } else {
                StatusCode::UNAUTHORIZED
            }
        );
    }
    let status = admin_json(&app, &actors.admin, "/api/admin/status").await;
    // 第 max 次失敗只觸發這個 IP 的鎖定；鎖定後的嘗試不寫事件
    assert_eq!(status["statistics"]["login_failures"], max);
    assert_eq!(status["statistics"]["account_locks"], 0);
    let locked = admin_json(
        &app,
        &actors.admin,
        "/api/admin/diagnostics?failure_class=account_locked",
    )
    .await;
    assert_eq!(locked["items"], json!([]));
}

#[sqlx::test]
async fn login_failure_reports_which_lock_level_was_reached(pool: PgPool) {
    use socrates_chat_backend::auth_store::{
        MAX_EMAIL_LOGIN_FAILURES, MAX_LOGIN_FAILURES, NewLocks, record_login_failure,
    };
    let now = chrono::Utc::now();
    for attempt in 1..=MAX_LOGIN_FAILURES {
        let locks = record_login_failure(&pool, "one-ip@example.com", "10.0.0.1", now)
            .await
            .unwrap();
        let expected = NewLocks {
            ip: attempt == MAX_LOGIN_FAILURES,
            account: false,
        };
        assert_eq!(locks, expected);
    }
    // 分散在多個 IP、每個 IP 都未達門檻：只有總數達到門檻時才是帳號層級鎖定
    for attempt in 1..=MAX_EMAIL_LOGIN_FAILURES {
        let ip = format!("10.1.{}.1", (attempt - 1) / (MAX_LOGIN_FAILURES - 1));
        let locks = record_login_failure(&pool, "spray@example.com", &ip, now)
            .await
            .unwrap();
        let expected = NewLocks {
            ip: false,
            account: attempt == MAX_EMAIL_LOGIN_FAILURES,
        };
        assert_eq!(locks, expected);
    }
}

#[sqlx::test]
async fn failures_for_unknown_accounts_are_capped_per_hour(pool: PgPool) {
    let app = test_app(pool.clone());
    let actors = seed_actors(&app).await;
    sqlx::query(
        "INSERT INTO diagnostic_events
            (request_id, kind, failure_class, function, route, status, error_code)
         SELECT gen_random_uuid(), 'login', 'invalid_credentials', 'password_login',
                '/api/auth/login', 401, 'invalid_credentials'
         FROM generate_series(1, 100)",
    )
    .execute(&pool)
    .await
    .unwrap();
    for _ in 0..3 {
        let email = format!("{}@example.com", Uuid::new_v4());
        let response = send(
            &app,
            json_req(
                "POST",
                "/api/auth/login",
                None,
                Some(json!({"account": email, "password": "wrong-synthetic-password"})),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let status = admin_json(&app, &actors.admin, "/api/admin/status").await;
    assert_eq!(status["statistics"]["login_failures"], 100);
}

#[sqlx::test]
async fn invalid_output_is_not_a_provider_failure(pool: PgPool) {
    let app = test_app(pool.clone());
    let actors = seed_actors(&app).await;
    sqlx::query(
        "INSERT INTO diagnostic_events
            (request_id, kind, failure_class, function, route, status, error_code,
             duration_ms, retry_count, model)
         VALUES (gen_random_uuid(), 'ai', 'invalid_output', 'summary', '<background>', 202,
                 'ai_unavailable', 20, 0, 'synthetic-model')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let status = admin_json(&app, &actors.admin, "/api/admin/status").await;
    assert_eq!(status["statistics"]["ai_status"], "ok");
    assert_eq!(status["statistics"]["ai_calls"], 1);
    assert_eq!(status["statistics"]["ai_failures"], 0);
    assert_eq!(status["statistics"]["ai_invalid_outputs"], 1);
    let listed = admin_json(
        &app,
        &actors.admin,
        "/api/admin/diagnostics?failure_class=invalid_output",
    )
    .await;
    assert_eq!(listed["items"].as_array().unwrap().len(), 1);
}

#[sqlx::test]
async fn diagnostic_storage_failure_does_not_change_login_or_service_status(pool: PgPool) {
    let app = test_app(pool.clone());
    let actors = seed_actors(&app).await;
    sqlx::query("ALTER TABLE diagnostic_events RENAME TO synthetic_unavailable")
        .execute(&pool)
        .await
        .unwrap();
    let response = send(
        &app,
        json_req(
            "POST",
            "/api/auth/login",
            None,
            Some(json!({"account": "missing@example.com", "password": "synthetic-password"})),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let status = admin_json(&app, &actors.admin, "/api/admin/status").await;
    assert_eq!(status["service"], "ok");
    assert_eq!(status["database"], "ok");
    assert_eq!(status["diagnostics_available"], false);
    assert!(status["statistics"].is_null());
    let response = send(&app, get("/api/admin/diagnostics", Some(&actors.admin))).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        json_body(response).await["error"]["code"],
        "diagnostics_unavailable"
    );
}

#[sqlx::test]
async fn provider_quota_is_distinct_from_rate_limiting_and_discards_provider_text(pool: PgPool) {
    use socrates_chat_backend::{AppState, ai::OpenAiProvider, key_pool::KeyPool};
    use std::sync::Arc;

    let mock = axum::Router::new().route(
        "/chat/completions",
        axum::routing::post(|| async {
            (
                StatusCode::TOO_MANY_REQUESTS,
                axum::Json(json!({
                    "error": {"code": "insufficient_quota", "message": "synthetic upstream secret"}
                })),
            )
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    let config = test_config();
    // 429 讓金鑰冷卻後重試；縮短冷卻，測試不必等預設的冷卻時間
    let keys = Arc::new(
        KeyPool::new(vec!["test-key".to_string()])
            .unwrap()
            .with_default_cooldown(std::time::Duration::from_millis(10)),
    );
    let ai = OpenAiProvider::new(&base, keys, "synthetic-provider-model");
    register_fixture_pool(&pool);
    let app = socrates_chat_backend::app(AppState::new(
        pool,
        config,
        Arc::new(FakeIdentity),
        Arc::new(ai),
    ));
    let actors = seed_actors(&app).await;
    let conversation = start_conversation(&app, &actors).await;
    let response = send(
        &app,
        json_req(
            "POST",
            &format!("/api/conversations/{conversation}/messages"),
            Some(&actors.student),
            Some(json!({"content": "synthetic prompt"})),
        ),
    )
    .await;
    let id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    let stream = text_body(response).await;
    assert!(stream.contains("ai_unavailable"));
    assert!(!stream.contains("synthetic upstream secret"));
    let body = admin_json(
        &app,
        &actors.admin,
        &format!("/api/admin/diagnostics?request_id={id}&failure_class=quota_exceeded"),
    )
    .await;
    // 學生回覆自動重試一次，兩次嘗試各記一筆
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    for item in items {
        assert_eq!(item["model"], "synthetic-provider-model");
    }
    assert!(!body.to_string().contains("synthetic upstream secret"));
    server.abort();
}

#[derive(Clone, Copy)]
enum Primary {
    /// HTTP 429 加上 `insufficient_quota`
    HttpQuota,
    /// 200 串流，還沒送出內容就帶 `insufficient_quota` 的錯誤區塊
    StreamQuota,
}

/// 診斷分類只供紀錄：主模型回報額度錯誤時，仍要換備援模型，學生照常收到回覆。
async fn quota_on_primary_still_uses_fallback(pool: PgPool, primary: Primary) {
    use axum::response::IntoResponse;
    use socrates_chat_backend::{AppState, ai::OpenAiProvider, key_pool::KeyPool};
    use std::sync::Arc;

    const CHUNK: &str = "data: {\"choices\":[{\"delta\":{\"content\":\"fallback reply\"}}]}\n\n\
        data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
    fn sse(body: &str) -> axum::response::Response {
        (
            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
            body.to_owned(),
        )
            .into_response()
    }
    let mock = axum::Router::new().route(
        "/chat/completions",
        axum::routing::post(move |axum::Json(body): axum::Json<Value>| async move {
            if body["model"] != "primary-model" {
                return sse(CHUNK);
            }
            match primary {
                Primary::HttpQuota => (
                    StatusCode::TOO_MANY_REQUESTS,
                    axum::Json(json!({"error": {"code": "insufficient_quota"}})),
                )
                    .into_response(),
                Primary::StreamQuota => sse(
                    "data: {\"error\":{\"code\":429,\"type\":\"insufficient_quota\",\"message\":\"x\"}}\n\n",
                ),
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    let config = test_config();
    let keys = Arc::new(
        KeyPool::new(vec!["test-key".to_string()])
            .unwrap()
            .with_default_cooldown(std::time::Duration::from_millis(10)),
    );
    let ai = OpenAiProvider::new(&base, keys, "primary-model")
        .with_fallbacks(vec!["fallback-model".to_owned()]);
    register_fixture_pool(&pool);
    let app = socrates_chat_backend::app(AppState::new(
        pool.clone(),
        config,
        Arc::new(FakeIdentity),
        Arc::new(ai),
    ));
    let actors = seed_actors(&app).await;
    let conversation = start_conversation(&app, &actors).await;
    let response = send(
        &app,
        json_req(
            "POST",
            &format!("/api/conversations/{conversation}/messages"),
            Some(&actors.student),
            Some(json!({"content": "synthetic prompt"})),
        ),
    )
    .await;
    let id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    let stream = text_body(response).await;
    assert!(stream.contains("fallback reply"), "{stream}");
    assert!(!stream.contains("ai_unavailable"));
    // 成功事件只供統計，但要記到實際使用的備援模型與這次換模型的重試
    let body = admin_json(
        &app,
        &actors.admin,
        &format!("/api/admin/diagnostics?request_id={id}"),
    )
    .await;
    assert_eq!(body["items"], json!([]));
    let (retries, model): (i32, String) = sqlx::query_as(
        "SELECT retry_count, model FROM diagnostic_events
         WHERE function = 'chat_reply' AND failure_class IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(model, "fallback-model");
    assert_eq!(retries, 1);
    server.abort();
}

#[sqlx::test]
async fn http_quota_on_the_primary_model_still_falls_back(pool: PgPool) {
    quota_on_primary_still_uses_fallback(pool, Primary::HttpQuota).await;
}

#[sqlx::test]
async fn streamed_quota_before_content_still_falls_back(pool: PgPool) {
    quota_on_primary_still_uses_fallback(pool, Primary::StreamQuota).await;
}
