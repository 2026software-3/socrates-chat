//! F-22.2 日誌機敏資料過濾（S-02.2、S-09.3）：日誌不得出現密碼、金鑰、聊天原文或電子郵件。
//! 以真正的 tracing 輸出擷取，走過登入、重設密碼與對話，其中包含 AI 服務回傳錯誤的路徑
//! （該路徑會寫日誌，用來證明擷取有效）。全部使用合成資料。
//! 這個檔案只能有一個測試：tracing 的全域訂閱者整個程序只能設定一次。

mod common;

use std::{
    io,
    sync::{Arc, Mutex},
};

use axum::{
    Json, Router,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::post,
};
use common::*;
use serde_json::json;
use socrates_chat_backend::{
    AppState,
    ai::OpenAiProvider,
    app,
    classroom_ai_store::{self, ModelRef},
    key_pool::KeyPool,
    teacher_key_crypto::KeyCipher,
};
use sqlx::PgPool;
use tracing_subscriber::fmt::MakeWriter;
use uuid::Uuid;

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
/// 教師 API key（F-40）與加密金鑰：任何日誌都不能出現。
const TEACHER_KEY: &str = "sk-synthetic-LOGS-1111-2222-3333-kkkk";
const REPLACED_TEACHER_KEY: &str = "sk-synthetic-LOGS-9999-8888-7777-rrrr";
const KEY_ENCRYPTION_SECRET: &str = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";

/// 教師 key 的供應商假服務：永遠回 401，並把收到的 key 與固定文字回聲在錯誤本文裡。
async fn rejecting_teacher_provider() -> String {
    let router = Router::new().fallback(|headers: axum::http::HeaderMap| async move {
        let echoed = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": {"message": format!("{PROVIDER_BODY} {echoed}")}})),
        )
            .into_response()
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{addr}/v1/")
}
const PURCHASE_DB_DETAIL: &str = "synthetic_part_private_detail";

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
    socrates_chat_backend::logging::json_subscriber()
        .with_writer(logs.clone())
        // 正式程序的 filter，並以最詳細的 `RUST_LOG=trace` 檢驗：genai 會在 trace 層級印出回應本文，
        // 一定要被擋掉（教師 key 的呼叫走 genai，F-40.2）
        .with_env_filter(socrates_chat_backend::logging::env_filter(Some("trace")))
        .init();

    let provider = OpenAiProvider::new(
        &failing_ai_base_url().await,
        Arc::new(KeyPool::new(vec!["SECRETAPIKEY".into()]).unwrap()),
        "some-model",
    );
    register_fixture_pool(&pool);
    let notification_pool = pool.clone();
    let mut config = test_config();
    config.teacher_key_cipher = Some(Arc::new(
        KeyCipher::parse(&format!("k1:{KEY_ENCRYPTION_SECRET}")).unwrap(),
    ));
    config.classroom_ai_routing = true;
    let app = app(
        AppState::new(pool, config, Arc::new(FakeIdentity), Arc::new(provider))
            .with_route_endpoint(rejecting_teacher_provider().await),
    );

    let a = seed_actors(&app).await;
    // 先以「過渡中」的教室走系統 key，維持下面既有的系統 AI 錯誤路徑斷言；教師 key 的路徑在最後
    sqlx::query("UPDATE classrooms SET ai_transitional = true")
        .execute(&notification_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO store_parts (id, name_zh_tw, name_en, name_es, price) VALUES ('synthetic_part', $1, 'Test part', 'Pieza de prueba', 30)")
        .bind(PURCHASE_DB_DETAIL).execute(&notification_pool).await.unwrap();
    sqlx::query("ALTER TABLE store_parts ADD CHECK (price <> 42)")
        .execute(&notification_pool)
        .await
        .unwrap();
    let rejected_price = send(
        &app,
        json_req(
            "PATCH",
            "/api/admin/parts/synthetic_part",
            Some(&a.admin),
            Some(json!({"price": 42, "enabled": true})),
        ),
    )
    .await;
    assert_json_error(
        rejected_price,
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal",
    )
    .await;

    let roster_response = send(
        &app,
        json_req(
            "POST",
            &format!("/api/classrooms/{}/roster/import", a.classroom),
            Some(&a.teacher),
            Some(json!({"text": "隱私測試姓名,98765432a\n隱私錯誤姓名,98765433b,invalid"})),
        ),
    )
    .await;
    assert_eq!(roster_response.status(), StatusCode::OK);
    let roster_result = json_body(roster_response).await;
    assert!(!roster_result.to_string().contains("隱私錯誤姓名"));
    assert!(!roster_result.to_string().contains("98765433"));
    let roster = json_body(
        send(
            &app,
            get(
                &format!("/api/classrooms/{}/students", a.classroom),
                Some(&a.teacher),
            ),
        )
        .await,
    )
    .await;
    let student = roster["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["student_number"] == "98765432A")
        .unwrap();
    let id = student["id"].as_str().unwrap();
    let rejected = send(
        &app,
        json_req(
            "DELETE",
            &format!("/api/students/{id}"),
            Some(&a.teacher),
            Some(json!({"confirm_student_number":"98765432B"})),
        ),
    )
    .await;
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    assert!(!json_body(rejected).await.to_string().contains("98765432"));
    // 名單外與錯誤密碼的登入嘗試
    let login_res = send(
        &app,
        json_req(
            "POST",
            "/api/auth/login",
            None,
            Some(json!({"email": "student@example.com", "password": WRONG_PASSWORD})),
        ),
    )
    .await;
    assert_eq!(login_res.status(), StatusCode::UNAUTHORIZED);
    let login_request_id = login_res.headers()[header::HeaderName::from_static("x-request-id")]
        .to_str()
        .unwrap()
        .to_string();
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

    let student = json_body(send(&app, get("/api/me", Some(&a.student))).await).await;
    let student_id: Uuid = student["id"].as_str().unwrap().parse().unwrap();
    let mut tx = notification_pool.begin().await.unwrap();
    socrates_chat_backend::student_notifications::record_student_number_correction(
        &mut tx, student_id,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let notices = json_body(send(&app, get("/api/notifications", Some(&a.student))).await).await;
    let notice_id = notices[0]["id"].as_str().unwrap();
    let read_path = format!("/api/notifications/{notice_id}/read");
    let read = send(&app, json_req("POST", &read_path, Some(&a.student), None)).await;
    assert_eq!(read.status(), StatusCode::NO_CONTENT);

    // 教師 key 的完整流程：新增、更換、教室設定、學生訊息走教師 key（供應商回 401 並回聲 key）、
    // 管理者檢視與刪除。任何一步都不能讓 key、密文或供應商回應本文進入日誌。
    let added = json_body(
        send(
            &app,
            json_req(
                "POST",
                "/api/teacher/keys",
                Some(&a.teacher),
                Some(json!({"provider": "openai", "key": TEACHER_KEY})),
            ),
        )
        .await,
    )
    .await;
    let key_id = added["id"].as_str().unwrap().to_owned();
    let duplicate = send(
        &app,
        json_req(
            "POST",
            "/api/teacher/keys",
            Some(&a.teacher),
            Some(json!({"provider": "openai", "key": TEACHER_KEY})),
        ),
    )
    .await;
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);
    let classroom: Uuid = a.classroom.parse().unwrap();
    classroom_ai_store::set(
        &notification_pool,
        classroom,
        ModelRef {
            provider: "openai".into(),
            model: "synthetic-model".into(),
        },
        vec![],
    )
    .await
    .unwrap();
    classroom_ai_store::clear_transitional(&notification_pool, classroom)
        .await
        .unwrap();
    let routed = start_conversation(&app, &a).await;
    let (_, events) = student_says_events(&app, &a, &routed, STUDENT_MESSAGE).await;
    let events = events.expect("SSE stream");
    let routed_error = events
        .iter()
        .find(|(name, _)| name == "error")
        .map(|(_, data)| data["code"].clone());
    assert_eq!(routed_error, Some(json!("classroom_ai_unavailable")));
    let replaced = send(
        &app,
        json_req(
            "PUT",
            &format!("/api/teacher/keys/{key_id}"),
            Some(&a.teacher),
            Some(json!({"key": REPLACED_TEACHER_KEY})),
        ),
    )
    .await;
    assert_eq!(replaced.status(), StatusCode::OK);
    let listed = send(&app, get("/api/admin/teacher-keys", Some(&a.admin))).await;
    assert_eq!(listed.status(), StatusCode::OK);
    let deleted = send(
        &app,
        json_req(
            "DELETE",
            &format!("/api/admin/teacher-keys/{key_id}"),
            Some(&a.admin),
            None,
        ),
    )
    .await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

    let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(!output.contains(student["student_number"].as_str().unwrap()));
    assert!(!output.contains(notice_id));
    let entries: Vec<serde_json::Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).expect("每行 tracing 輸出都應為 JSON"))
        .collect();
    assert!(!entries.is_empty(), "應擷取到 JSON 日誌");
    assert!(entries.iter().any(|entry| {
        entry["fields"]["message"] == "database error" && entry["fields"]["class"] == "23514"
    }));
    assert!(!output.contains(PURCHASE_DB_DETAIL));
    let request_fields = entries
        .iter()
        .filter_map(|entry| entry.get("fields"))
        .find(|fields| {
            fields.get("message") == Some(&json!("request completed"))
                && fields.get("request_id") == Some(&json!(login_request_id))
        })
        .expect("每個 request 都應以回應的 x-request-id 留下完成紀錄");
    // 沒有錯誤的 request 不輸出 error_code（與 API 的 null 一致），而不是空字串
    let completions: Vec<_> = entries
        .iter()
        .filter_map(|entry| entry.get("fields"))
        .filter(|fields| fields.get("message") == Some(&json!("request completed")))
        .collect();
    assert!(
        completions
            .iter()
            .all(|f| f.get("error_code") != Some(&json!("")))
    );
    assert!(
        completions
            .iter()
            .any(|f| { f.get("status") == Some(&json!(200)) && f.get("error_code").is_none() })
    );
    assert_eq!(request_fields["method"], "POST");
    assert_eq!(request_fields["route"], "/api/auth/login");
    assert_eq!(request_fields["status"], 401);
    assert_eq!(request_fields["error_code"], "invalid_credentials");
    assert!(request_fields["duration_ms"].is_u64());
    // 串流回應的標頭是 200，錯誤藏在最後的 error 事件：完成紀錄要等本文結束，並帶出該 error code
    let streamed = completions
        .iter()
        .find(|f| f.get("route") == Some(&json!("/api/conversations/{id}/messages")))
        .expect("串流回應結束後也要留下完成紀錄");
    assert_eq!(streamed["status"], 200);
    assert_eq!(streamed["error_code"], "ai_unavailable");
    let provider_entry = entries
        .iter()
        .find(|entry| entry["fields"]["message"] == "AI provider returned an error status")
        .expect("應擷取到 AI provider 錯誤事件");
    assert_eq!(
        provider_entry["span"]["route"],
        "/api/conversations/{id}/messages"
    );
    assert!(
        provider_entry["span"]["request_id"]
            .as_str()
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok()),
        "request 內部事件應繼承可用的 request ID"
    );
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
        ("教師 API key", TEACHER_KEY),
        ("教師 API key 片段", "1111-2222-3333"),
        ("更換後的教師 API key", REPLACED_TEACHER_KEY),
        ("金鑰加密用的密鑰", KEY_ENCRYPTION_SECRET),
        ("Google client secret", "test-secret"),
        ("學生電子郵件", "student@example.com"),
        ("教師電子郵件", "teacher@example.com"),
        ("重設對象電子郵件", "reset@example.com"),
        ("管理者電子郵件", "admin@example.com"),
        ("臨時密碼", temporary_password.as_str()),
        ("匯入姓名", "隱私測試姓名"),
        ("匯入學號", "98765432"),
        ("錯誤列姓名", "隱私錯誤姓名"),
        ("錯誤列學號", "98765433"),
    ] {
        assert!(!output.contains(secret), "日誌不應包含：{name}");
    }
}
