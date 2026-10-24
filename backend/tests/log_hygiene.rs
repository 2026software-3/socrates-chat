//! F-22.2 日誌機敏資料過濾（S-02.2、S-09.3）：日誌不得出現密碼、金鑰、聊天原文或電子郵件。
//! 以真正的 tracing 輸出擷取，走過登入、重設密碼與對話，其中包含 AI 服務回傳錯誤的路徑
//! （該路徑會寫日誌，用來證明擷取有效）。全部使用合成資料。
//! 這個檔案只能有一個測試：tracing 的全域訂閱者整個程序只能設定一次。

mod common;

use std::{
    io,
    sync::{Arc, Mutex},
};

use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::post};
use common::*;
use serde_json::json;
use socrates_chat_backend::{AppState, ai::OpenAiProvider, app, key_pool::KeyPool};
use sqlx::PgPool;
use tracing_subscriber::fmt::MakeWriter;

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

const STUDENT_MESSAGE: &str = "SECRETMESSAGE老師我覺得存在主義很難懂";
const WRONG_PASSWORD: &str = "SECRETPASSWORD-wrong-1";
/// 假的 AI 服務在錯誤回應裡回聲這段文字，日誌不能把回應本文帶出來。
const PROVIDER_BODY: &str = "SECRETPROVIDERBODY";

/// 一個永遠回 500 的假 OpenAI 服務。
async fn failing_ai_base_url() -> String {
    let router = Router::new().route(
        "/chat/completions",
        post(|| async {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": PROVIDER_BODY})),
            )
                .into_response()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{addr}")
}

#[sqlx::test]
async fn logs_never_contain_passwords_emails_keys_or_message_text(pool: PgPool) {
    let logs = Captured::default();
    tracing_subscriber::fmt()
        .with_writer(logs.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .init();

    let provider = OpenAiProvider::new(
        &failing_ai_base_url().await,
        Arc::new(KeyPool::new(vec!["SECRETAPIKEY".into()]).unwrap()),
        "some-model",
    );
    let app = app(AppState::new(
        pool,
        test_config(),
        Arc::new(FakeIdentity),
        Arc::new(provider),
    ));

    let a = seed_actors(&app).await;
    // 名單外與錯誤密碼的登入嘗試
    send(
        &app,
        json_req(
            "POST",
            "/api/auth/login",
            None,
            Some(json!({"email": "student@example.com", "password": WRONG_PASSWORD})),
        ),
    )
    .await;
    // 管理者替另一個信箱重設密碼（回應裡有臨時密碼；重設學生本人會讓其 session 失效）
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/users/reset-password",
            Some(&a.admin),
            Some(json!({"email": "reset@example.com"})),
        ),
    )
    .await;
    let temporary_password = json_body(res).await["temporary_password"]
        .as_str()
        .unwrap()
        .to_string();
    // 學生送出訊息，AI 服務回 500
    let conv = start_conversation(&app, &a).await;
    student_says(&app, &a, &conv, STUDENT_MESSAGE).await;

    let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(
        output.contains("AI provider returned an error status"),
        "擷取到會寫日誌的路徑，才能證明下面的斷言有意義：{output}"
    );
    // 失敗訊息只印名稱，不印值（值本身是機敏資料，也不把整段日誌印出來）
    for (name, secret) in [
        ("學生訊息", STUDENT_MESSAGE),
        ("錯誤密碼", WRONG_PASSWORD),
        ("AI 服務回應本文", PROVIDER_BODY),
        ("API key", "SECRETAPIKEY"),
        ("Google client secret", "test-secret"),
        ("學生電子郵件", "student@example.com"),
        ("教師電子郵件", "teacher@example.com"),
        ("重設對象電子郵件", "reset@example.com"),
        ("管理者電子郵件", "admin@example.com"),
        ("臨時密碼", temporary_password.as_str()),
    ] {
        assert!(!output.contains(secret), "日誌不應包含：{name}");
    }
}
