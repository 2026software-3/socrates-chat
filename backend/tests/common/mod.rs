//! 整合測試共用工具：假的身分服務、測試設定與登入輔助函式。全部使用合成資料。
#![allow(dead_code)]

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{Request, Response, StatusCode, header},
};
use futures_util::{StreamExt, stream};
use http_body_util::BodyExt;
use serde_json::Value;
use socrates_chat_backend::{
    AppState,
    ai::{AiError, AiProvider, AiRequest, AiStream},
    app,
    config::Config,
    identity::{AuthRequest, Identity, IdentityError, IdentityProvider},
    key_pool::KeyPool,
};
use sqlx::{Connection, PgPool};
use tower::ServiceExt;

pub const APP_URL: &str = "https://app.test";

/// 假的 Google：授權碼格式為 `ok:<sub>:<email>`、`unverified:<sub>:<email>`，其他一律失敗。
pub struct FakeIdentity;

#[async_trait]
impl IdentityProvider for FakeIdentity {
    fn start(&self) -> AuthRequest {
        let n = uuid::Uuid::new_v4();
        AuthRequest {
            url: format!("https://idp.test/authorize?state=st-{n}"),
            state: format!("st-{n}"),
            nonce: format!("nonce-{n}"),
            pkce_verifier: format!("verifier-{n}"),
        }
    }

    async fn finish(
        &self,
        code: &str,
        _nonce: &str,
        _pkce_verifier: &str,
    ) -> Result<Identity, IdentityError> {
        let mut parts = code.splitn(3, ':');
        let kind = parts.next().unwrap_or_default();
        let sub = parts.next().ok_or(IdentityError)?;
        let email = parts.next().ok_or(IdentityError)?;
        match kind {
            "ok" | "unverified" => Ok(Identity {
                sub: sub.to_string(),
                email: email.to_string(),
                email_verified: kind == "ok",
                name: Some(format!("User {sub}")),
            }),
            _ => Err(IdentityError),
        }
    }
}

pub fn test_config() -> Config {
    Config {
        database_url: String::new(),
        app_base_url: APP_URL.to_string(),
        google_client_id: "test-client".to_string(),
        google_client_secret: "test-secret".to_string(),
        google_redirect_url: format!("{APP_URL}/api/auth/google/callback"),
        admin_emails: vec!["admin@example.com".to_string()],
        admin_initial_password: None,
        cookie_secure: true,
        listen_addr: "127.0.0.1:0".to_string(),
        trusted_proxies: vec![],
        login_rate_burst: socrates_chat_backend::login_rate_limit::DEFAULT_BURST,
        login_rate_replenish: socrates_chat_backend::login_rate_limit::DEFAULT_REPLENISH,
        ai_usage_max: 1000,
        ai_usage_window: Duration::from_secs(3600),
        frontend_dir: None,
        openai_keys: Arc::new(KeyPool::new(vec!["test-key".to_string()]).unwrap()),
        openai_model: "test-model".to_string(),
        openai_base_url: "http://127.0.0.1:1".to_string(),
        openai_stt_base_url: None,
        openai_tts_base_url: None,
        openai_fallback_models: vec![],
        openai_stt_model: None,
        openai_tts_model: None,
        openai_tts_voice: "alloy".to_string(),
        wrap_up_turn: 3,
        max_turns: 5,
        // 縮短逾時讓逾時測試很快跑完
        ai_first_token_timeout: Duration::from_millis(200),
        ai_total_timeout: Duration::from_millis(1000),
        request_timeout: Duration::from_secs(10),
        demo_mode: false,
    }
}

/// 一次 AI 呼叫的腳本。
pub enum Script {
    /// 依序給出這些片段後正常結束
    Reply(Vec<&'static str>),
    /// 直接失敗（尚未串流任何內容）
    Fail,
    /// 給出這些片段後中途失敗
    FailAfter(Vec<&'static str>),
    /// 永遠不回應
    Hang,
    /// 每個片段前都先等一段時間，模擬回覆產生到一半
    Paced(Vec<&'static str>, std::time::Duration),
}

/// 假的 AI：依序執行腳本（用完後一律失敗），並記錄收到的請求。
#[derive(Default)]
pub struct FakeAi {
    scripts: Mutex<VecDeque<Script>>,
    pub requests: Mutex<Vec<AiRequest>>,
}

impl FakeAi {
    pub fn with(scripts: Vec<Script>) -> Arc<Self> {
        Arc::new(Self {
            scripts: Mutex::new(scripts.into()),
            requests: Mutex::default(),
        })
    }

    pub fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

#[async_trait]
impl AiProvider for FakeAi {
    async fn stream_chat(&self, req: AiRequest) -> Result<AiStream, AiError> {
        self.requests.lock().unwrap().push(req);
        let script = self.scripts.lock().unwrap().pop_front();
        match script {
            Some(Script::Reply(chunks)) => {
                Ok(stream::iter(chunks.into_iter().map(|c| Ok(c.to_string()))).boxed())
            }
            Some(Script::FailAfter(chunks)) => Ok(stream::iter(
                chunks
                    .into_iter()
                    .map(|c| Ok(c.to_string()))
                    .chain([Err(AiError)]),
            )
            .boxed()),
            Some(Script::Hang) => std::future::pending().await,
            Some(Script::Paced(chunks, delay)) => Ok(stream::iter(chunks)
                .then(move |c| async move {
                    tokio::time::sleep(delay).await;
                    Ok(c.to_string())
                })
                .boxed()),
            Some(Script::Fail) | None => Err(AiError),
        }
    }
}

pub fn test_app(pool: PgPool) -> Router {
    test_app_with_ai(pool, FakeAi::with(vec![]))
}

pub fn test_app_with_ai(pool: PgPool, ai: Arc<FakeAi>) -> Router {
    test_app_with_config(pool, ai, test_config())
}

thread_local! {
    /// 這個測試最近一次建立 app 時的資料庫連線設定，給 `import_roster` 夾具模擬「學生自己填了電子郵件」。
    /// 只存連線設定、不存連線池：連線池留在 thread-local 會讓測試資料庫的連線拖到執行緒結束才釋放。
    /// sqlx 測試在自己的執行緒上用單執行緒 runtime 跑，所以不同測試不會互相覆蓋。
    static FIXTURE_DB: std::cell::RefCell<Option<std::sync::Arc<sqlx::postgres::PgConnectOptions>>> =
        const { std::cell::RefCell::new(None) };
}

/// 不經 `test_app` 自己組 `AppState` 的測試要先呼叫，讓 `import_roster` 找得到資料庫。
pub fn register_fixture_pool(pool: &PgPool) {
    FIXTURE_DB.with(|o| *o.borrow_mut() = Some(pool.connect_options()));
}

pub fn test_app_with_config(pool: PgPool, ai: Arc<FakeAi>, config: Config) -> Router {
    register_fixture_pool(&pool);
    app(AppState::new(pool, config, Arc::new(FakeIdentity), ai))
}

/// 解析 SSE 回應內容為 (event 名稱, data) 清單。
pub fn parse_sse(body: &str) -> Vec<(String, Value)> {
    body.split("\n\n")
        .filter_map(|block| {
            let mut name = None;
            let mut data = None;
            for line in block.lines() {
                if let Some(v) = line.strip_prefix("event:") {
                    name = Some(v.trim().to_string());
                } else if let Some(v) = line.strip_prefix("data:") {
                    data = serde_json::from_str(v.trim()).ok();
                }
            }
            Some((name?, data?))
        })
        .collect()
}

pub async fn text_body(res: Response<Body>) -> String {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8_lossy(&bytes).to_string()
}

pub async fn send(app: &Router, req: Request<Body>) -> Response<Body> {
    app.clone().oneshot(req).await.unwrap()
}

pub async fn json_body(res: Response<Body>) -> Value {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

/// 錯誤回應必須是統一的 JSON 錯誤格式：指定的狀態碼與 `code`，且 `request_id` 與 `x-request-id` 一致。
pub async fn assert_json_error(res: Response<Body>, status: StatusCode, code: &str) {
    assert_eq!(res.status(), status);
    let rid = res.headers()["x-request-id"].to_str().unwrap().to_string();
    assert!(
        res.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("application/json")
    );
    let body = json_body(res).await;
    assert_eq!(body["error"]["code"], code);
    assert_eq!(body["error"]["request_id"], rid);
}

/// 一般請求逾時很短的設定。登入等前置步驟要用一般設定的 app 先做好，再換成這個。
pub fn short_timeout() -> Config {
    Config {
        request_timeout: Duration::from_millis(300),
        ai_first_token_timeout: Duration::from_secs(3),
        ai_total_timeout: Duration::from_secs(5),
        ..test_config()
    }
}

pub fn location(res: &Response<Body>) -> String {
    res.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_string()
}

/// 取出回應中 `sid` cookie 的 `name=value` 部分，供後續請求帶上。
pub fn session_cookie(res: &Response<Body>) -> Option<String> {
    res.headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with("sid=") && !v.starts_with("sid=;"))
        .map(|v| v.split(';').next().unwrap().to_string())
}

/// 走完整個登入流程，回傳 session cookie（`sid=...`）。
pub async fn login_as(app: &Router, sub: &str, email: &str) -> String {
    let res = send(
        app,
        Request::get("/api/auth/google/login")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let url = location(&res);
    let state = url.split("state=").nth(1).unwrap().to_string();
    let res = send(
        app,
        Request::get(format!(
            "/api/auth/google/callback?code=ok:{sub}:{email}&state={state}"
        ))
        .body(Body::empty())
        .unwrap(),
    )
    .await;
    session_cookie(&res).expect("login should set a session cookie")
}

pub fn get(path: &str, cookie: Option<&str>) -> Request<Body> {
    let mut b = Request::get(path);
    if let Some(c) = cookie {
        b = b.header(header::COOKIE, c);
    }
    b.body(Body::empty()).unwrap()
}

/// 四種身分的 session cookie（合成資料）。
pub struct Actors {
    pub admin: String,
    pub teacher: String,
    pub student: String,
    /// 名單外、非教師、非管理者
    pub outsider: String,
    /// 教師建立的教室 id；`student` 在這間教室的名單內
    pub classroom: String,
}

/// 教師建立教室，回傳教室 id。
pub async fn create_classroom(app: &Router, cookie: &str, name: &str) -> String {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/classrooms",
            Some(cookie),
            Some(serde_json::json!({"name": name})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    json_body(res).await["id"].as_str().unwrap().to_string()
}

/// 把文字名單匯入指定教室（`cookie` 為該教室的教師或管理者），回傳回應。
/// 匯入只收「姓名,學號」。其他功能的測試夾具仍以信箱列出學生（Google 登入靠信箱比對帳號）：
/// 單獨一個信箱會產生固定的合成學號，`姓名,學號,信箱` 則原樣使用；匯入成功後，
/// 直接在資料庫替這些帳號填上信箱，模擬學生自己填寫電子郵件（匯入本身不會設定信箱）。
pub async fn import_roster(
    app: &Router,
    cookie: &str,
    classroom: &str,
    text: &str,
) -> Response<Body> {
    import_roster_as(app, Some(cookie), classroom, text).await
}

/// 同 `import_roster`，但 `cookie` 可為空（測試未登入的情況）。
pub async fn import_roster_as(
    app: &Router,
    cookie: Option<&str>,
    classroom: &str,
    text: &str,
) -> Response<Body> {
    let (text, emails) = fixture_roster(text);
    let response = send(
        app,
        json_req(
            "POST",
            &format!("/api/classrooms/{classroom}/roster/import"),
            cookie,
            Some(serde_json::json!({"text": text})),
        ),
    )
    .await;
    if response.status().is_success() {
        let options = FIXTURE_DB
            .with(|o| o.borrow().clone())
            .expect("先呼叫 test_app 建立 app");
        let mut conn = sqlx::PgConnection::connect_with(&options).await.unwrap();
        for (student_id, email) in emails {
            sqlx::query("UPDATE users SET email = $1 WHERE student_id = $2 AND email IS NULL")
                .bind(email)
                .bind(student_id)
                .execute(&mut conn)
                .await
                .unwrap();
        }
        conn.close().await.unwrap();
    }
    response
}

/// 夾具由信箱導出的固定合成學號。
pub fn fixture_student_id(email: &str) -> String {
    use sha2::{Digest, Sha256};
    let email = email.trim().to_ascii_lowercase();
    if email == "student@example.com" {
        return "10000000A".to_string();
    }
    let digest = Sha256::digest(email.as_bytes());
    let number = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]) % 100_000_000;
    format!("{number:08}A")
}

/// 回傳送給匯入 API 的兩欄文字，以及要替帳號填上的（學號, 信箱）。
fn fixture_roster(text: &str) -> (String, Vec<(String, String)>) {
    let mut lines = Vec::new();
    let mut emails = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split(',').map(str::trim).collect();
        if fields.len() == 1 && !line.contains('@') {
            // 刻意寫壞的行原樣送出，讓匯入把它當錯誤列
            lines.push(line.to_string());
            continue;
        }
        let (name, student_id, email) = if fields.len() >= 2 {
            (
                fields[0].to_string(),
                fields[1].to_ascii_uppercase(),
                fields.get(2).copied().unwrap_or("").to_ascii_lowercase(),
            )
        } else {
            let email = line.trim().to_ascii_lowercase();
            ("測試學生".to_string(), fixture_student_id(&email), email)
        };
        lines.push(format!("{name},{student_id}"));
        if !email.is_empty() {
            emails.push((student_id, email));
        }
    }
    (lines.join("\n"), emails)
}

/// 教師建立一個題目，回傳 id。
pub async fn create_topic(app: &Router, a: &Actors, title: &str) -> String {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/topics",
            Some(&a.teacher),
            Some(serde_json::json!({"title": title, "description": "你會拉桿嗎？"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    json_body(res).await["id"].as_str().unwrap().to_string()
}

/// 教師以指定題目建立活動（草稿），回傳 id。
pub async fn create_activity(app: &Router, a: &Actors, title: &str, topic_id: &str) -> String {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(serde_json::json!({
                "title": title,
                "description": "你會拉桿嗎？",
                "topic_id": topic_id,
                "classroom_id": a.classroom,
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    json_body(res).await["id"].as_str().unwrap().to_string()
}

/// 教師以指定題目建立並發布一個活動，回傳 id。
pub async fn publish_activity(app: &Router, a: &Actors, title: &str, topic_id: &str) -> String {
    let act = create_activity(app, a, title, topic_id).await;
    let res = send(
        app,
        json_req(
            "POST",
            &format!("/api/activities/{act}/publish"),
            Some(&a.teacher),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    act
}

/// 教師建立題目與活動並發布，回傳活動 id（題目標題與活動標題相同）。
pub async fn published_activity_with_topic(app: &Router, a: &Actors, title: &str) -> String {
    let topic = create_topic(app, a, title).await;
    publish_activity(app, a, title, &topic).await
}

/// 學生從活動開始一場對話，回傳對話 id。
pub async fn start_from_activity(app: &Router, cookie: &str, activity_id: &str) -> String {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/conversations",
            Some(cookie),
            Some(serde_json::json!({"activity_id": activity_id})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    json_body(res).await["id"].as_str().unwrap().to_string()
}

/// 管理者把另一位學生加進修課名單並讓他登入，回傳 session cookie。
pub async fn add_student(app: &Router, a: &Actors, sub: &str, email: &str) -> String {
    let res = import_roster(app, &a.admin, &a.classroom, email).await;
    assert!(res.status().is_success());
    login_as(app, sub, email).await
}

/// 教師建立並發布一個活動，學生從中開始一場對話，回傳對話 id。
pub async fn start_conversation(app: &Router, a: &Actors) -> String {
    let act = published_activity_with_topic(app, a, "電車難題").await;
    start_from_activity(app, &a.student, &act).await
}

/// 學生在對話中送出一則訊息。成功時回應是 SSE：讀完整條串流（等 AI 回覆結束），
/// 回傳狀態與第一個 `message` 事件的內容；失敗時回傳 JSON 錯誤。
/// 注意：這會觸發一次 AI 呼叫（消耗假 AI 的腳本）；只想放一則訊息當前置條件時用 `seed_student_message`。
pub async fn student_says(app: &Router, a: &Actors, conv: &str, text: &str) -> (StatusCode, Value) {
    let (status, events) = student_says_events(app, a, conv, text).await;
    match events {
        Ok(events) => (
            status,
            events
                .into_iter()
                .find(|(n, _)| n == "message")
                .map(|(_, d)| d)
                .unwrap_or(Value::Null),
        ),
        Err(body) => (status, body),
    }
}

/// 同上，但回傳 SSE 的全部事件（成功）或 JSON 錯誤本文（失敗）。
pub async fn student_says_events(
    app: &Router,
    a: &Actors,
    conv: &str,
    text: &str,
) -> (StatusCode, Result<Vec<(String, Value)>, Value>) {
    let res = send(
        app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/messages"),
            Some(&a.student),
            Some(serde_json::json!({"content": text})),
        ),
    )
    .await;
    let status = res.status();
    if status == StatusCode::OK {
        (status, Ok(parse_sse(&text_body(res).await)))
    } else {
        (status, Err(json_body(res).await))
    }
}

/// 直接寫入一則學生訊息當作測試前置條件：不經過 API、不呼叫 AI、不佔用假 AI 的腳本。
pub async fn seed_student_message(pool: &PgPool, conv: &str, text: &str) {
    let id: uuid::Uuid = conv.parse().unwrap();
    sqlx::query("INSERT INTO messages (conversation_id, role, content) VALUES ($1, 'student', $2)")
        .bind(id)
        .bind(text)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE conversations SET turn_count = turn_count + 1 WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
}

/// 管理者新增教師 `teacher@example.com`，教師登入並建立一間教室；回傳教師 cookie 與教室 id。
pub async fn seed_classroom(app: &Router, admin: &str) -> (String, String) {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(admin),
            Some(serde_json::json!({"email": "teacher@example.com"})),
        ),
    )
    .await;
    assert!(res.status().is_success());
    let teacher = login_as(app, "sub-teacher", "teacher@example.com").await;
    let classroom = create_classroom(app, &teacher, "測試教室").await;
    (teacher, classroom)
}

/// 建立管理者、教師（含一間教室）、教室名單內學生與名單外帳號並登入。
pub async fn seed_actors(app: &Router) -> Actors {
    let admin = login_as(app, "sub-admin", "admin@example.com").await;
    let (teacher, classroom) = seed_classroom(app, &admin).await;
    let res = import_roster(
        app,
        &admin,
        &classroom,
        "學生,10000000A,student@example.com",
    )
    .await;
    assert!(res.status().is_success());
    Actors {
        student: login_as(app, "sub-student", "student@example.com").await,
        outsider: login_as(app, "sub-outsider", "outsider@example.com").await,
        teacher,
        admin,
        classroom,
    }
}

/// 帶 `Origin: APP_URL` 的 JSON 請求（符合 CSRF 檢查）。
pub fn json_req(
    method: &str,
    path: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> Request<Body> {
    let mut b = Request::builder()
        .method(method)
        .uri(path)
        .header(header::ORIGIN, APP_URL)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(c) = cookie {
        b = b.header(header::COOKIE, c);
    }
    b.body(match body {
        Some(v) => Body::from(v.to_string()),
        None => Body::empty(),
    })
    .unwrap()
}
