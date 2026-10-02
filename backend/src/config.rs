//! 設定一律來自環境變數（S-08.3）；秘密不進版控、不進前端。

use std::{env, time::Duration};

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    /// 前端／API 共用網域的根網址，例如 `https://socrates.example.edu`（S-08.1）。
    /// 登入完成後導回此處，也是 CSRF `Origin` 檢查的比對對象。
    pub app_base_url: String,
    pub google_client_id: String,
    pub google_client_secret: String,
    pub google_redirect_url: String,
    /// 首次登入即成為管理者的信箱（小寫）。
    pub admin_emails: Vec<String>,
    /// session cookie 是否加 `Secure`；本機 http 開發可設為 `false`。
    pub cookie_secure: bool,
    pub listen_addr: String,
    pub openai_api_key: String,
    /// 具體模型尚未決定（S-03.1），因此沒有預設值，必須明確設定。
    pub openai_model: String,
    pub openai_base_url: String,
    /// 第幾回合起 AI 引導學生收尾（S-03.3）。
    pub wrap_up_turn: u32,
    /// 回合上限（S-03.3）：第幾回合時直接結束。
    pub max_turns: u32,
    /// AI 首個 token 逾時（S-03.5）。
    pub ai_first_token_timeout: Duration,
    /// AI 完整回覆上限（S-03.5）。
    pub ai_total_timeout: Duration,
}

#[derive(Debug)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ConfigError {}

fn required(name: &str) -> Result<String, ConfigError> {
    env::var(name)
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| ConfigError(format!("missing environment variable {name}")))
}

fn parse_or<T: std::str::FromStr>(name: &str, default: T) -> Result<T, ConfigError> {
    match env::var(name) {
        Ok(v) if !v.is_empty() => v
            .parse()
            .map_err(|_| ConfigError(format!("invalid value for {name}"))),
        _ => Ok(default),
    }
}

/// 以逗號分隔的信箱清單，去空白並轉小寫。
pub fn parse_admin_emails(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

impl Config {
    /// 回覆產生的 claim 超過多久視為殘留（秒）：兩次嘗試的上限再加一點餘裕。
    pub fn generation_stale_secs(&self) -> f64 {
        self.ai_total_timeout.as_secs_f64() * 2.0 + 10.0
    }

    pub fn from_env() -> Result<Self, ConfigError> {
        let app_base_url = required("APP_BASE_URL")?.trim_end_matches('/').to_string();
        Ok(Self {
            database_url: required("DATABASE_URL")?,
            google_client_id: required("GOOGLE_CLIENT_ID")?,
            google_client_secret: required("GOOGLE_CLIENT_SECRET")?,
            google_redirect_url: env::var("GOOGLE_REDIRECT_URL")
                .unwrap_or_else(|_| format!("{app_base_url}/api/auth/google/callback")),
            admin_emails: parse_admin_emails(&env::var("ADMIN_EMAILS").unwrap_or_default()),
            cookie_secure: env::var("COOKIE_SECURE")
                .map(|v| v != "false")
                .unwrap_or(true),
            listen_addr: env::var("LISTEN_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".into()),
            openai_api_key: required("OPENAI_API_KEY")?,
            openai_model: required("OPENAI_MODEL")?,
            openai_base_url: env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".into()),
            wrap_up_turn: parse_or("WRAP_UP_TURN", 15)?,
            max_turns: parse_or("MAX_TURNS", 20)?,
            ai_first_token_timeout: Duration::from_secs(parse_or(
                "AI_FIRST_TOKEN_TIMEOUT_SECS",
                15,
            )?),
            ai_total_timeout: Duration::from_secs(parse_or("AI_TOTAL_TIMEOUT_SECS", 60)?),
            app_base_url,
        })
    }
}
