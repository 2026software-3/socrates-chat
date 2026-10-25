//! S-05.4 與 F-10.5-SEC：辨識成功或失敗後，原始音訊不得留在資料庫、磁碟、錯誤回應或日誌；
//! 語音互動只保存逐字稿，且逐字稿依 S-02.2 視為敏感欄位（防止越權存取與日誌外洩）。

mod common;

use std::{
    io,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{Request, StatusCode, header},
    response::IntoResponse,
    routing::post,
};
use common::Script::*;
use common::*;
use serde_json::json;
use socrates_chat_backend::{AppState, app, voice::OpenAiVoice};
use sqlx::PgPool;
use tracing_subscriber::fmt::MakeWriter;

const AUDIO_SENTINEL: &[u8] = b"RAW_AUDIO_SECRET_SENTINEL_7f19";
const PROVIDER_ERROR_SENTINEL: &str = "SYNTHETIC_STT_INTERNAL_ERROR_9a8b";
const TRANSCRIPT_SENTINEL: &str = "TRANSCRIPT_SECRET_SENTINEL_我認為不應該拉桿";

const AI_REPLY: [&str; 2] = [
    "為什麼你認為不應該拉桿？\n<<<META>>>\n",
    "{\"question_type\":\"reason\",\"advance\":false,\"reason\":\"尚未說明理由\"}",
];

#[derive(Clone, Default)]
struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

struct LogSink(Arc<Mutex<Vec<u8>>>);

impl io::Write for LogSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for CapturedLogs {
    type Writer = LogSink;

    fn make_writer(&'a self) -> Self::Writer {
        LogSink(self.0.clone())
    }
}

type CapturedRequests = Arc<Mutex<Vec<Vec<u8>>>>;

async fn spawn_fake_openai(status: StatusCode) -> (String, CapturedRequests) {
    async fn handle(
        State((status, captured)): State<(StatusCode, CapturedRequests)>,
        req: Request<Body>,
    ) -> axum::response::Response {
        let bytes: Bytes = axum::body::to_bytes(req.into_body(), usize::MAX)
            .await
            .unwrap();
        captured.lock().unwrap().push(bytes.to_vec());
        if status.is_success() {
            (status, axum::Json(json!({"text": TRANSCRIPT_SENTINEL}))).into_response()
        } else {
            (status, PROVIDER_ERROR_SENTINEL).into_response()
        }
    }

    let captured = CapturedRequests::default();
    let app = Router::new()
        .route("/audio/transcriptions", post(handle))
        .with_state((status, captured.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), captured)
}

fn app_with_openai_voice(pool: PgPool, base_url: &str) -> Router {
    let mut config = test_config();
    config.openai_base_url = base_url.to_string();
    config.openai_stt_model = Some("stt-test".into());
    let voice = Arc::new(OpenAiVoice::from_config(&config));
    let ai = FakeAi::with(vec![Reply(AI_REPLY.to_vec()), Reply(AI_REPLY.to_vec())]);
    app(AppState::new(pool, config, Arc::new(FakeIdentity), ai).with_voice(voice))
}

fn audio_request(cookie: Option<&str>, audio: &[u8]) -> Request<Body> {
    let mut b = Request::post("/api/voice/transcribe?language=zh-TW")
        .header(header::ORIGIN, APP_URL)
        .header(header::CONTENT_TYPE, "audio/webm");
    if let Some(c) = cookie {
        b = b.header(header::COOKIE, c);
    }
    b.body(Body::from(audio.to_vec())).unwrap()
}

/// 掃描 public schema 的每個資料表（整列轉成文字），確認原始音訊沒有出現在任何地方。
async fn assert_audio_absent_from_every_table(pool: &PgPool) {
    let hex_sentinel: String = AUDIO_SENTINEL.iter().map(|b| format!("{b:02x}")).collect();
    let plain_sentinel = String::from_utf8_lossy(AUDIO_SENTINEL);
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables
         WHERE table_schema = 'public' AND table_type = 'BASE TABLE'",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    assert!(!tables.is_empty(), "expected tables to scan");

    let mut scanned_rows = 0_usize;
    for table in tables {
        let rows: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT t::text FROM \"{table}\" t"
        )))
        .fetch_all(pool)
        .await
        .unwrap();
        scanned_rows += rows.len();
        for row in rows {
            assert!(
                !row.contains(plain_sentinel.as_ref()) && !row.contains(&hex_sentinel),
                "raw audio found in table {table}"
            );
        }
    }
    assert!(scanned_rows > 0, "expected seeded rows to scan");
}

/// 確認工作目錄下沒有殘留包含原始音訊的檔案。
fn assert_audio_absent_from_working_dir() {
    let entries = std::fs::read_dir(".").unwrap();
    for entry in entries.flatten() {
        if let Ok(ft) = entry.file_type()
            && ft.is_file()
            && let Ok(bytes) = std::fs::read(entry.path())
        {
            assert!(
                !bytes
                    .windows(AUDIO_SENTINEL.len())
                    .any(|w| w == AUDIO_SENTINEL),
                "raw audio found on disk at {:?}",
                entry.path()
            );
        }
    }
}

#[sqlx::test]
async fn audio_is_not_persisted_or_logged_and_transcript_is_protected(pool: PgPool) {
    let logs = CapturedLogs::default();
    tracing_subscriber::fmt()
        .with_writer(logs.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .init();

    let (success_base, success_requests) = spawn_fake_openai(StatusCode::OK).await;
    let success_app = app_with_openai_voice(pool.clone(), &success_base);
    let actors = seed_actors(&success_app).await;
    let other_student = add_student(&success_app, &actors, "sub-other", "other@example.com").await;

    // 1. 未授權與越權角色上傳音訊：應遭拒絕，不呼叫外部服務，錯誤回應不洩漏音訊
    for (cookie, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(actors.outsider.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.teacher.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.admin.as_str()), StatusCode::FORBIDDEN),
    ] {
        let res = send(&success_app, audio_request(cookie, AUDIO_SENTINEL)).await;
        assert_eq!(res.status(), expected);
        let text = text_body(res).await;
        assert!(!text.contains(&*String::from_utf8_lossy(AUDIO_SENTINEL)));
    }
    assert!(
        success_requests.lock().unwrap().is_empty(),
        "unauthorized requests must not reach STT provider"
    );

    // 2. 學生正常辨識成功：回傳逐字稿
    let success = send(
        &success_app,
        audio_request(Some(&actors.student), AUDIO_SENTINEL),
    )
    .await;
    assert_eq!(success.status(), StatusCode::OK);
    assert_eq!(
        json_body(success).await,
        json!({"text": TRANSCRIPT_SENTINEL})
    );
    assert!(
        String::from_utf8_lossy(&success_requests.lock().unwrap()[0])
            .contains(&*String::from_utf8_lossy(AUDIO_SENTINEL))
    );

    // 3. 辨識失敗：回傳 502 voice_failed，錯誤回應不洩漏原始音訊或上游錯誤本文
    let (failure_base, failure_requests) =
        spawn_fake_openai(StatusCode::INTERNAL_SERVER_ERROR).await;
    let failure_app = app_with_openai_voice(pool.clone(), &failure_base);
    let failure = send(
        &failure_app,
        audio_request(Some(&actors.student), AUDIO_SENTINEL),
    )
    .await;
    assert_eq!(failure.status(), StatusCode::BAD_GATEWAY);
    let failure_text = text_body(failure).await;
    let failure_json: serde_json::Value = serde_json::from_str(&failure_text).unwrap();
    assert_eq!(failure_json["error"]["code"], "voice_failed");
    assert!(!failure_text.contains(&*String::from_utf8_lossy(AUDIO_SENTINEL)));
    assert!(!failure_text.contains(PROVIDER_ERROR_SENTINEL));
    assert!(
        String::from_utf8_lossy(&failure_requests.lock().unwrap()[0])
            .contains(&*String::from_utf8_lossy(AUDIO_SENTINEL))
    );

    // 4. 語音互動只保存逐字稿（記錄來源），且逐字稿依 S-02.2 視為敏感欄位（防止越權讀取）
    let conv = start_conversation(&success_app, &actors).await;
    for source in ["speech-openai", "speech-browser"] {
        let res = send(
            &success_app,
            json_req(
                "POST",
                &format!("/api/conversations/{conv}/messages"),
                Some(&actors.student),
                Some(json!({"content": TRANSCRIPT_SENTINEL, "source": source})),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let _ = text_body(res).await;
    }

    let saved: Vec<(String, String)> = sqlx::query_as(
        "SELECT content, source FROM messages WHERE conversation_id = $1 AND role = 'student' ORDER BY seq",
    )
    .bind(conv.parse::<uuid::Uuid>().unwrap())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        saved,
        vec![
            (TRANSCRIPT_SENTINEL.to_string(), "speech-openai".to_string()),
            (
                TRANSCRIPT_SENTINEL.to_string(),
                "speech-browser".to_string()
            ),
        ]
    );

    // 其他學生、名單外、教師、管理者與未登入者皆無法越權讀取該場對話的逐字稿
    for (cookie, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(actors.outsider.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.teacher.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.admin.as_str()), StatusCode::FORBIDDEN),
        (Some(other_student.as_str()), StatusCode::NOT_FOUND),
    ] {
        let res = send(
            &success_app,
            get(&format!("/api/conversations/{conv}"), cookie),
        )
        .await;
        assert_eq!(res.status(), expected);
        let body = text_body(res).await;
        assert!(!body.contains(TRANSCRIPT_SENTINEL));
    }

    // 5. 確認資料庫、磁碟與日誌皆未保存原始音訊，且日誌未洩漏逐字稿或上游內部錯誤
    assert_audio_absent_from_every_table(&pool).await;
    assert_audio_absent_from_working_dir();

    let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(
        output.contains("speech-to-text returned an error status"),
        "expected to capture OpenAiVoice's failure log; got: {output}"
    );
    assert!(!output.contains(&*String::from_utf8_lossy(AUDIO_SENTINEL)));
    assert!(!output.contains(PROVIDER_ERROR_SENTINEL));
    assert!(!output.contains(TRANSCRIPT_SENTINEL));
}
