//! 設定一律來自環境變數（S-08.3）；秘密不進版控、不進前端。

use std::env;

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

/// 以逗號分隔的信箱清單，去空白並轉小寫。
pub fn parse_admin_emails(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

impl Config {
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
            app_base_url,
        })
    }
}
