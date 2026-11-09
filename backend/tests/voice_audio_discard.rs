//! S-05.4／F-10.5-SEC（Issue #210）：原始音訊不得留在資料庫、日誌或磁碟，只保存逐字稿。
//!
//! 涵蓋兩條會碰到音訊的路徑：
//! - 語音備援上傳 `POST /api/voice/transcribe`（辨識成功、辨識失敗、被拒絕的上傳）；
//! - cascade 語音對話的 WebSocket 轉送（瀏覽器 → 後端 → cascade，正常結束與對方突然斷線）。
//!
//! 每個測試用自己的獨特音訊內容（含隨機值），事後在三個地方找它的原文、十六進位與 base64 形式：
//! (a) 資料庫每個資料表的每一列、(b) tracing 日誌、(c) 暫存目錄裡本次測試期間改過的檔案。
//! 最後有「正向對照」測試，確認這三個掃描器放進音訊時確實會報告，避免掃描器壞掉卻一直通過。
//! 全部使用合成資料。
//!
//! 已知限制：(c) 只能掃暫存目錄，無法證明程式沒有寫到磁碟上別的地方；
//! 「程式從不寫檔」另由 `backend/src/voice.rs`、`cascade.rs` 沒有任何檔案寫入的程式碼審查佐證。

mod common;

use std::{
    io,
    net::SocketAddr,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, SystemTime},
};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{Request, StatusCode, header},
    response::IntoResponse,
    routing::post,
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
};
use common::*;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use socrates_chat_backend::{AppState, app, config::Config, voice::OpenAiVoice};
use sqlx::PgPool;
use tokio::time::{sleep, timeout};
use tokio_tungstenite::tungstenite::{
    Message, client::IntoClientRequest, http::header as ws_header,
};
use tracing_subscriber::fmt::MakeWriter;
use uuid::Uuid;

// ---------------------------------------------------------------- 日誌擷取

#[derive(Clone, Default)]
struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

struct LogSink(Arc<Mutex<Vec<u8>>>);

impl io::Write for LogSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend_from_slice(bytes);
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

/// 全程式共用一份日誌擷取（TRACE 等級）。用全域 subscriber 而不是 thread-local，
/// 這樣不管 runtime 是單執行緒或多執行緒、被測程式在哪個 task 寫日誌都擷取得到；
/// 各測試的音訊內容各自獨特，並行測試之間不會互相誤判。
fn logs() -> &'static CapturedLogs {
    static LOGS: OnceLock<CapturedLogs> = OnceLock::new();
    LOGS.get_or_init(|| {
        let logs = CapturedLogs::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(logs.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("no other global tracing subscriber in this test binary");
        logs
    })
}

fn log_text() -> String {
    String::from_utf8_lossy(&logs().0.lock().unwrap_or_else(|e| e.into_inner())).into_owned()
}

// ------------------------------------------------------------ 獨特音訊與掃描

/// 這次測試的「原始音訊」：獨特且帶隨機值，不會與其他測試或殘留檔案混淆。
fn audio_bytes() -> Vec<u8> {
    format!("RAW_AUDIO_SECRET_SENTINEL_{}", Uuid::new_v4().simple()).into_bytes()
}

/// 要找的形式：原文、十六進位（資料庫 bytea 的文字形式）、以及 base64（不論前面有 0–2 個位元組的偏移）。
fn needles(audio: &[u8]) -> Vec<(String, String)> {
    let mut out = vec![
        (
            "plain".to_string(),
            String::from_utf8_lossy(audio).into_owned(),
        ),
        (
            "hex".to_string(),
            audio.iter().map(|b| format!("{b:02x}")).collect(),
        ),
    ];
    // base64 把每 3 個位元組編成 4 個字元，偏移不同編出來的字元也不同。
    // 前面補 shift 個位元組後，開頭有幾個字元會混到補的位元；去掉它們與結尾可能不完整的 2 個字元，
    // 剩下的就是不受偏移影響的片段。
    for (shift, drop_front) in [(0_usize, 0_usize), (1, 2), (2, 3)] {
        let mut padded = vec![0u8; shift];
        padded.extend_from_slice(audio);
        let encoded = STANDARD_NO_PAD.encode(&padded);
        let piece = &encoded[drop_front..encoded.len() - 2];
        out.push((format!("base64+{shift}"), piece.to_string()));
    }
    out
}

fn found_in(haystack: &[u8], needles: &[(String, String)]) -> Vec<String> {
    needles
        .iter()
        .filter(|(_, n)| {
            let n = n.as_bytes();
            !n.is_empty() && haystack.windows(n.len()).any(|w| w == n)
        })
        .map(|(label, _)| label.clone())
        .collect()
}

/// (a) 掃描 public schema 的每個資料表（整列轉成文字）；回傳「資料表: 形式」的命中清單。
async fn scan_database(pool: &PgPool, needles: &[(String, String)]) -> Vec<String> {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables
         WHERE table_schema = 'public' AND table_type = 'BASE TABLE'",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    assert!(!tables.is_empty(), "expected tables to scan");
    let mut hits = Vec::new();
    for table in tables {
        let rows: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT t::text FROM \"{table}\" t"
        )))
        .fetch_all(pool)
        .await
        .unwrap();
        for row in rows {
            for form in found_in(row.as_bytes(), needles) {
                hits.push(format!("{table}: {form}"));
            }
        }
    }
    hits
}

/// (b) 掃描到目前為止擷取到的全部日誌。
fn scan_logs(needles: &[(String, String)]) -> Vec<String> {
    found_in(log_text().as_bytes(), needles)
}

/// (c) 掃描暫存目錄裡 `since` 之後改過的檔案內容（最多往下 4 層，略過符號連結與讀不了的檔案）。
fn scan_temp_files(since: SystemTime, needles: &[(String, String)]) -> Vec<String> {
    fn walk(
        dir: &Path,
        depth: usize,
        since: SystemTime,
        needles: &[(String, String)],
        out: &mut Vec<String>,
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.path().symlink_metadata() else {
                continue;
            };
            if meta.is_dir() && depth < 4 {
                walk(&entry.path(), depth + 1, since, needles, out);
            } else if meta.is_file()
                && meta.len() <= 64 * 1024 * 1024
                && meta.modified().is_ok_and(|m| m >= since)
                && let Ok(bytes) = std::fs::read(entry.path())
            {
                for form in found_in(&bytes, needles) {
                    out.push(format!("{}: {form}", entry.path().display()));
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(&std::env::temp_dir(), 0, since, needles, &mut out);
    out
}

fn test_started() -> SystemTime {
    // 往前抓 2 秒，容許檔案系統時間的粒度
    SystemTime::now() - Duration::from_secs(2)
}

/// 三個地方都不得出現這段音訊。
async fn assert_audio_nowhere(pool: &PgPool, audio: &[u8], since: SystemTime) {
    let needles = needles(audio);
    assert_eq!(scan_database(pool, &needles).await, Vec::<String>::new());
    assert_eq!(scan_logs(&needles), Vec::<String>::new());
    assert_eq!(scan_temp_files(since, &needles), Vec::<String>::new());
}

// ------------------------------------------------------- 假的 OpenAI（辨識）

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
            (status, axum::Json(json!({"text": "合成逐字稿"}))).into_response()
        } else {
            (status, "synthetic provider failure").into_response()
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
    register_fixture_pool(&pool);
    app(AppState::new(pool, config, Arc::new(FakeIdentity), FakeAi::with(vec![])).with_voice(voice))
}

fn audio_request(
    cookie: Option<&str>,
    uri: &str,
    content_type: &str,
    audio: &[u8],
) -> Request<Body> {
    let mut req = Request::post(uri)
        .header(header::ORIGIN, APP_URL)
        .header(header::CONTENT_TYPE, content_type);
    if let Some(cookie) = cookie {
        req = req.header(header::COOKIE, cookie);
    }
    req.body(Body::from(audio.to_vec())).unwrap()
}

const TRANSCRIBE: &str = "/api/voice/transcribe?language=zh-TW";

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

// ------------------------------------------------------------ 語音備援上傳

#[sqlx::test]
async fn transcribe_discards_audio_after_success_and_after_failure(pool: PgPool) {
    logs();
    let since = test_started();
    let audio = audio_bytes();

    let (success_base, success_requests) = spawn_fake_openai(StatusCode::OK).await;
    let success_app = app_with_openai_voice(pool.clone(), &success_base);
    let actors = seed_actors(&success_app).await;
    let success = send(
        &success_app,
        audio_request(Some(&actors.student), TRANSCRIBE, "audio/webm", &audio),
    )
    .await;
    assert_eq!(success.status(), StatusCode::OK);
    assert_eq!(json_body(success).await, json!({"text": "合成逐字稿"}));
    // 對照：音訊確實有送到辨識服務，所以下面「找不到」才有意義
    assert!(contains(&success_requests.lock().unwrap()[0], &audio));

    let (failure_base, failure_requests) =
        spawn_fake_openai(StatusCode::INTERNAL_SERVER_ERROR).await;
    let failure_app = app_with_openai_voice(pool.clone(), &failure_base);
    let failure = send(
        &failure_app,
        audio_request(Some(&actors.student), TRANSCRIBE, "audio/webm", &audio),
    )
    .await;
    assert_eq!(failure.status(), StatusCode::BAD_GATEWAY);
    let failure_body = json_body(failure).await;
    assert_eq!(failure_body["error"]["code"], "voice_failed");
    assert!(
        !failure_body
            .to_string()
            .contains(&*String::from_utf8_lossy(&audio)),
        "錯誤回應不得帶音訊"
    );
    assert!(contains(&failure_requests.lock().unwrap()[0], &audio));

    // 對照：失敗路徑的日誌確實有被擷取到
    assert!(
        log_text().contains("speech-to-text returned an error status"),
        "expected to capture OpenAiVoice's failure log"
    );

    assert_audio_nowhere(&pool, &audio, since).await;
}

#[sqlx::test]
async fn rejected_uploads_never_reach_the_provider_or_storage(pool: PgPool) {
    logs();
    let since = test_started();
    let audio = audio_bytes();

    let (base, requests) = spawn_fake_openai(StatusCode::OK).await;
    let app = app_with_openai_voice(pool.clone(), &base);
    let a = seed_actors(&app).await;

    let cases: [(Option<&str>, &str, &str, StatusCode); 5] = [
        (None, TRANSCRIBE, "audio/webm", StatusCode::UNAUTHORIZED),
        (
            Some(&a.teacher),
            TRANSCRIBE,
            "audio/webm",
            StatusCode::FORBIDDEN,
        ),
        (
            Some(&a.admin),
            TRANSCRIBE,
            "audio/webm",
            StatusCode::FORBIDDEN,
        ),
        (
            Some(&a.outsider),
            TRANSCRIBE,
            "audio/webm",
            StatusCode::FORBIDDEN,
        ),
        // 登入的學生但格式不對：不是音訊、語言不支援，一樣不得轉給辨識服務或留下
        (
            Some(&a.student),
            TRANSCRIBE,
            "text/plain",
            StatusCode::BAD_REQUEST,
        ),
    ];
    for (cookie, uri, content_type, expected) in cases {
        let res = send(&app, audio_request(cookie, uri, content_type, &audio)).await;
        assert_eq!(res.status(), expected, "{cookie:?} {content_type}");
    }
    let res = send(
        &app,
        audio_request(
            Some(&a.student),
            "/api/voice/transcribe?language=fr",
            "audio/webm",
            &audio,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    assert!(
        requests.lock().unwrap().is_empty(),
        "被拒絕的上傳不得送到辨識服務"
    );
    assert_audio_nowhere(&pool, &audio, since).await;
}

// ------------------------------------------------- cascade WebSocket 轉送

/// 假的 cascade：收下一條連線，把收到的文字訊息記下來。
/// `drop_after_audio` 為真時，收到音訊後不送 Close 就直接斷線（模擬辨識端失敗）。
async fn spawn_fake_cascade(drop_after_audio: bool) -> (String, Arc<Mutex<Vec<String>>>) {
    let received = Arc::new(Mutex::new(Vec::<String>::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = received.clone();
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
        while let Some(Ok(frame)) = socket.next().await {
            match frame {
                Message::Text(text) => {
                    let is_audio = text.as_str().contains("input_audio_buffer.append");
                    seen.lock().unwrap().push(text.as_str().to_string());
                    if is_audio && drop_after_audio {
                        return; // 直接丟掉連線，不送 Close
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });
    (format!("ws://{addr}/v1/realtime"), received)
}

/// 學生從瀏覽器串流一段音訊，cascade 正常結束或突然斷線；之後音訊不得留在任何地方。
async fn relayed_audio_is_not_kept(pool: PgPool, drop_after_audio: bool) {
    logs();
    let since = test_started();
    let audio = audio_bytes();

    let (cascade_url, received) = spawn_fake_cascade(drop_after_audio).await;
    register_fixture_pool(&pool);
    let config = Config {
        cascade_realtime_url: Some(cascade_url),
        cascade_llm_token: Some("synthetic-llm-secret".into()),
        ..test_config()
    };
    let origin = config.app_base_url.clone();
    let router = app(AppState::new(
        pool.clone(),
        config,
        Arc::new(FakeIdentity),
        FakeAi::with(vec![]),
    ));
    let a = seed_actors(&router).await;
    let conv = start_conversation(&router, &a).await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let served = router.clone();
    tokio::spawn(async move {
        axum::serve(
            listener,
            served.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });

    let mut request = format!("ws://{addr}/api/conversations/{conv}/voice")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert(ws_header::COOKIE, a.student.parse().unwrap());
    request
        .headers_mut()
        .insert(ws_header::ORIGIN, origin.parse().unwrap());
    let (socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let (mut tx, mut rx) = socket.split();

    tx.send(Message::text(
        json!({
            "type": "input_audio_buffer.append",
            "event_id": "e0",
            "audio": STANDARD.encode(&audio),
        })
        .to_string(),
    ))
    .await
    .unwrap();

    // 對照：音訊確實被轉給 cascade，所以下面「找不到」才有意義
    timeout(Duration::from_secs(10), async {
        loop {
            if received
                .lock()
                .unwrap()
                .iter()
                .any(|f| f.contains(&STANDARD.encode(&audio)))
            {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the relay should forward the audio frame to the cascade");

    if !drop_after_audio {
        tx.send(Message::Close(None)).await.unwrap();
    }
    // 等轉送結束（對方斷線或收到 Close 後，後端會關閉瀏覽器這一側）
    timeout(Duration::from_secs(10), async {
        while let Some(frame) = rx.next().await {
            if matches!(frame, Ok(Message::Close(_)) | Err(_)) {
                break;
            }
        }
    })
    .await
    .expect("the relay should end");

    assert_audio_nowhere(&pool, &audio, since).await;
}

#[sqlx::test]
async fn cascade_relay_keeps_no_audio_after_a_normal_close(pool: PgPool) {
    relayed_audio_is_not_kept(pool, false).await;
}

#[sqlx::test]
async fn cascade_relay_keeps_no_audio_when_the_cascade_drops_the_connection(pool: PgPool) {
    relayed_audio_is_not_kept(pool, true).await;
}

// ---------------------------------------------------- 掃描器的正向對照

#[sqlx::test]
async fn the_database_scanner_finds_planted_audio_in_every_form(pool: PgPool) {
    let audio = audio_bytes();
    sqlx::query("CREATE TABLE audio_probe (plain text, raw bytea, encoded text)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO audio_probe VALUES ($1, $2, $3)")
        .bind(String::from_utf8_lossy(&audio).into_owned())
        .bind(&audio)
        .bind(STANDARD.encode(&audio))
        .execute(&pool)
        .await
        .unwrap();

    let hits = scan_database(&pool, &needles(&audio)).await;
    let forms: Vec<&str> = hits
        .iter()
        .filter(|h| h.starts_with("audio_probe: "))
        .map(|h| h.trim_start_matches("audio_probe: "))
        .collect();
    for expected in ["plain", "hex", "base64+0"] {
        assert!(forms.contains(&expected), "missing {expected} in {hits:?}");
    }
}

#[test]
fn the_log_scanner_finds_audio_written_to_the_log() {
    logs();
    let audio = audio_bytes();
    assert!(scan_logs(&needles(&audio)).is_empty());
    tracing::info!(leaked = %String::from_utf8_lossy(&audio), "synthetic leak");
    assert!(scan_logs(&needles(&audio)).contains(&"plain".to_string()));
}

#[test]
fn the_temp_scanner_finds_audio_written_to_a_temp_file() {
    let since = test_started();
    let audio = audio_bytes();
    let dir = std::env::temp_dir().join(format!("audio-probe-{}", Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("clip.webm"), &audio).unwrap();

    let hits = scan_temp_files(since, &needles(&audio));
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        hits.iter().any(|h| h.contains("clip.webm")),
        "scanner missed the planted file: {hits:?}"
    );
}

#[test]
fn the_base64_needles_match_the_audio_at_any_alignment() {
    let audio = audio_bytes();
    let needles = needles(&audio);
    for prefix in 0..3 {
        let mut stream = vec![7u8; prefix];
        stream.extend_from_slice(&audio);
        stream.extend_from_slice(b"tail");
        let encoded = STANDARD.encode(&stream);
        assert!(
            !found_in(encoded.as_bytes(), &needles).is_empty(),
            "alignment {prefix} not detected"
        );
    }
}
