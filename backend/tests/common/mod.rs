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
};
use sqlx::PgPool;
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
        cookie_secure: true,
        listen_addr: "127.0.0.1:0".to_string(),
        openai_api_key: "test-key".to_string(),
        openai_model: "test-model".to_string(),
        openai_base_url: "http://127.0.0.1:1".to_string(),
        wrap_up_turn: 3,
        max_turns: 5,
        // 縮短逾時讓逾時測試很快跑完
        ai_first_token_timeout: Duration::from_millis(200),
        ai_total_timeout: Duration::from_millis(1000),
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

pub fn test_app_with_config(pool: PgPool, ai: Arc<FakeAi>, config: Config) -> Router {
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
}

/// 教師建立並發布一個活動，學生從中開始一場對話，回傳對話 id。
pub async fn start_conversation(app: &Router, a: &Actors) -> String {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(serde_json::json!({"title": "電車難題", "description": "你會拉桿嗎？"})),
        ),
    )
    .await;
    let act = json_body(res).await["id"].as_str().unwrap().to_string();
    send(
        app,
        json_req(
            "POST",
            &format!("/api/activities/{act}/publish"),
            Some(&a.teacher),
            None,
        ),
    )
    .await;
    let res = send(
        app,
        json_req(
            "POST",
            "/api/conversations",
            Some(&a.student),
            Some(serde_json::json!({"activity_id": act})),
        ),
    )
    .await;
    json_body(res).await["id"].as_str().unwrap().to_string()
}

/// 學生在對話中送出一則訊息，回傳回應內容。
pub async fn student_says(app: &Router, a: &Actors, conv: &str, text: &str) -> (StatusCode, Value) {
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
    (status, json_body(res).await)
}

/// 建立管理者、教師、名單內學生與名單外帳號並登入。
pub async fn seed_actors(app: &Router) -> Actors {
    let admin = login_as(app, "sub-admin", "admin@example.com").await;
    for (path, body) in [
        (
            "/api/admin/teachers",
            serde_json::json!({"email": "teacher@example.com"}),
        ),
        (
            "/api/roster/import",
            serde_json::json!({"text": "student@example.com"}),
        ),
    ] {
        let res = send(app, json_req("POST", path, Some(&admin), Some(body))).await;
        assert!(res.status().is_success());
    }
    Actors {
        teacher: login_as(app, "sub-teacher", "teacher@example.com").await,
        student: login_as(app, "sub-student", "student@example.com").await,
        outsider: login_as(app, "sub-outsider", "outsider@example.com").await,
        admin,
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
