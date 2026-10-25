//! S-05.4 與 F-10.5-SEC：辨識成功或失敗後，原始音訊不得留在資料庫、磁碟、錯誤回應或日誌；
//! 語音互動只保存逐字稿，且逐字稿依 S-02.2 視為敏感欄位（防止越權存取與日誌外洩）。

mod common;

use std::{
    io,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{Request, StatusCode, header},
    response::IntoResponse,
    routing::post,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use common::*;
use serde_json::json;
use socrates_chat_backend::{AppState, app, voice::OpenAiVoice};
use sqlx::PgPool;
use tracing_subscriber::fmt::MakeWriter;

const PROVIDER_ERROR_SENTINEL: &str = "SYNTHETIC_STT_INTERNAL_ERROR_9a8b";
const TRANSCRIPT_SENTINEL: &str = "TRANSCRIPT_SECRET_SENTINEL_我認為不應該拉桿";

/// 每次執行都不同的假音訊：不會出現在原始碼或編譯產物裡，因此磁碟掃描不會誤判。
fn audio_sentinel() -> &'static [u8] {
    static AUDIO: OnceLock<Vec<u8>> = OnceLock::new();
    AUDIO.get_or_init(|| format!("RAW_AUDIO_SECRET_{}", uuid::Uuid::new_v4().simple()).into_bytes())
}

/// 原始音訊可能被寫成的樣子：原文、十六進位、`Vec<u8>` 的偵錯輸出（十進位陣列）與 base64。
fn audio_forms() -> Vec<String> {
    let audio = audio_sentinel();
    vec![
        String::from_utf8_lossy(audio).into_owned(),
        audio.iter().map(|b| format!("{b:02x}")).collect(),
        audio
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        STANDARD.encode(audio),
    ]
}

fn contains_audio(text: &str) -> bool {
    audio_forms().iter().any(|form| text.contains(form))
}

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

/// 整個測試檔共用一個 tracing subscriber（只能設定一次），回傳它收集到的日誌。
fn captured_logs() -> &'static CapturedLogs {
    static LOGS: OnceLock<CapturedLogs> = OnceLock::new();
    LOGS.get_or_init(|| {
        let logs = CapturedLogs::default();
        let _ = tracing_subscriber::fmt()
            .with_writer(logs.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .try_init();
        logs
    })
}

fn log_output() -> String {
    String::from_utf8_lossy(&captured_logs().0.lock().unwrap()).into_owned()
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
    let ai = FakeAi::with(vec![
        Script::Reply(AI_REPLY.to_vec()),
        Script::Reply(AI_REPLY.to_vec()),
    ]);
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
            assert!(!contains_audio(&row), "raw audio found in table {table}");
        }
    }
    assert!(scanned_rows > 0, "expected seeded rows to scan");
}

/// 遞迴掃描目錄，確認沒有檔案含有原始音訊（略過符號連結、建置產物與過大的檔案）。
fn assert_audio_absent_under(dir: &Path, depth: usize) {
    const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
    if depth > 6 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() {
            if !matches!(entry.file_name().to_str(), Some("target" | ".git")) {
                assert_audio_absent_under(&path, depth + 1);
            }
        } else if file_type.is_file()
            && entry.metadata().is_ok_and(|m| m.len() <= MAX_FILE_BYTES)
            && let Ok(bytes) = std::fs::read(&path)
        {
            assert!(
                !bytes
                    .windows(audio_sentinel().len())
                    .any(|w| w == audio_sentinel()),
                "raw audio found on disk at {path:?}"
            );
        }
    }
}

/// 確認工作目錄與系統暫存目錄下都沒有殘留包含原始音訊的檔案。
fn assert_audio_absent_from_disk() {
    assert_audio_absent_under(Path::new("."), 0);
    assert_audio_absent_under(&std::env::temp_dir(), 0);
}

fn assert_audio_absent_from_logs() {
    assert!(!contains_audio(&log_output()), "raw audio found in logs");
}

/// 學生以語音來源送出逐字稿，回傳整條 SSE 事件。
async fn post_transcript(
    app: &Router,
    actors: &Actors,
    conv: &str,
    source: &str,
) -> Vec<(String, serde_json::Value)> {
    let res = send(
        app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/messages"),
            Some(&actors.student),
            Some(json!({"content": TRANSCRIPT_SENTINEL, "source": source})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    parse_sse(&text_body(res).await)
}

#[sqlx::test]
async fn successful_transcription_leaves_no_raw_audio_anywhere(pool: PgPool) {
    captured_logs();
    let (base, requests) = spawn_fake_openai(StatusCode::OK).await;
    let app = app_with_openai_voice(pool.clone(), &base);
    let actors = seed_actors(&app).await;

    let res = send(&app, audio_request(Some(&actors.student), audio_sentinel())).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_body(res).await, json!({"text": TRANSCRIPT_SENTINEL}));
    // 音訊確實只轉送給辨識服務
    assert!(requests.lock().unwrap().iter().any(|req| {
        req.windows(audio_sentinel().len())
            .any(|w| w == audio_sentinel())
    }));

    assert_audio_absent_from_every_table(&pool).await;
    assert_audio_absent_from_disk();
    assert_audio_absent_from_logs();
    assert!(!log_output().contains(TRANSCRIPT_SENTINEL));
}

#[sqlx::test]
async fn failed_transcription_hides_audio_and_provider_error(pool: PgPool) {
    captured_logs();
    let (base, requests) = spawn_fake_openai(StatusCode::INTERNAL_SERVER_ERROR).await;
    let app = app_with_openai_voice(pool.clone(), &base);
    let actors = seed_actors(&app).await;

    let res = send(&app, audio_request(Some(&actors.student), audio_sentinel())).await;
    assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
    let text = text_body(res).await;
    let body: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["error"]["code"], "voice_failed");
    assert!(!contains_audio(&text));
    assert!(!text.contains(PROVIDER_ERROR_SENTINEL));
    assert!(requests.lock().unwrap().iter().any(|req| {
        req.windows(audio_sentinel().len())
            .any(|w| w == audio_sentinel())
    }));

    assert_audio_absent_from_every_table(&pool).await;
    assert_audio_absent_from_disk();
    let output = log_output();
    assert!(
        output.contains("speech-to-text returned an error status"),
        "expected to capture OpenAiVoice's failure log; got: {output}"
    );
    assert!(!contains_audio(&output));
    assert!(!output.contains(PROVIDER_ERROR_SENTINEL));
}

#[sqlx::test]
async fn speech_interaction_stores_only_the_transcript_and_completes_the_turn(pool: PgPool) {
    captured_logs();
    let (base, _) = spawn_fake_openai(StatusCode::OK).await;
    let app = app_with_openai_voice(pool.clone(), &base);
    let actors = seed_actors(&app).await;
    let conv = start_conversation(&app, &actors).await;

    for source in ["speech-openai", "speech-browser"] {
        let events = post_transcript(&app, &actors, &conv, source).await;
        let names: Vec<&str> = events.iter().map(|(n, _)| n.as_str()).collect();
        assert!(
            names.contains(&"done"),
            "{source}: AI turn incomplete: {names:?}"
        );
        assert!(
            !names.contains(&"error"),
            "{source}: stream errored: {names:?}"
        );
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

    // 擁有者讀得回自己的逐字稿
    let res = send(
        &app,
        get(&format!("/api/conversations/{conv}"), Some(&actors.student)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(text_body(res).await.contains(TRANSCRIPT_SENTINEL));

    assert_audio_absent_from_every_table(&pool).await;
    assert!(!log_output().contains(TRANSCRIPT_SENTINEL));
}

#[sqlx::test]
async fn transcript_is_not_readable_by_anyone_but_its_owner(pool: PgPool) {
    captured_logs();
    let (base, _) = spawn_fake_openai(StatusCode::OK).await;
    let app = app_with_openai_voice(pool.clone(), &base);
    let actors = seed_actors(&app).await;
    let other_student = add_student(&app, &actors, "sub-other", "other@example.com").await;
    let conv = start_conversation(&app, &actors).await;
    post_transcript(&app, &actors, &conv, "speech-openai").await;

    // 單一對話：其他學生、名單外、教師、管理者與未登入者都讀不到逐字稿
    for (cookie, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(actors.outsider.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.teacher.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.admin.as_str()), StatusCode::FORBIDDEN),
        (Some(other_student.as_str()), StatusCode::NOT_FOUND),
    ] {
        let res = send(&app, get(&format!("/api/conversations/{conv}"), cookie)).await;
        assert_eq!(res.status(), expected);
        assert!(!text_body(res).await.contains(TRANSCRIPT_SENTINEL));
    }

    // 教師與管理者實際會看到對話衍生資料的路徑（總結與列表）也不得帶出逐字稿
    for cookie in [&actors.teacher, &actors.admin] {
        for path in [
            "/api/summaries".to_string(),
            format!("/api/conversations/{conv}/summary"),
        ] {
            let res = send(&app, get(&path, Some(cookie))).await;
            assert!(!res.status().is_server_error(), "{path}: {}", res.status());
            assert!(
                !text_body(res).await.contains(TRANSCRIPT_SENTINEL),
                "transcript leaked via {path}"
            );
        }
    }
}
