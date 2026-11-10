//! 設定一律來自環境變數（S-08.3）；秘密不進版控、不進前端。

use std::{sync::Arc, time::Duration};

use crate::key_pool::{KeyPool, merge_keys, parse_key_lines};

#[derive(Debug, Clone)]
pub struct Config {
    /// 前端／API 共用網域的根網址，例如 `https://socrates.example.edu`（S-08.1）。
    /// 登入完成後導回此處，也是 CSRF `Origin` 檢查的比對對象。
    pub app_base_url: String,
    pub google_client_id: String,
    pub google_client_secret: String,
    pub google_redirect_url: String,
    /// 首次登入即成為管理者的信箱（小寫）。
    pub admin_emails: Vec<String>,
    /// 首位管理者內建登入的初始密碼（S-01.3）；沒設定就不建立內建管理者帳號，只能用 Google 登入。
    pub admin_initial_password: Option<String>,
    /// session cookie 是否加 `Secure`；本機 http 開發可設為 `false`。
    pub cookie_secure: bool,
    /// 受信任的反向代理網段（`TRUSTED_PROXIES`，逗號分隔的 CIDR）。只有 TCP 對端落在這些網段時，
    /// 才採用 `X-Forwarded-For` 作為來源 IP；預設為空，一律用 TCP 對端（S-08.2 登入限流）。
    pub trusted_proxies: Vec<crate::client_ip::Cidr>,
    /// 登入請求頻率限制（`LOGIN_RATE_BURST`、`LOGIN_RATE_REPLENISH_SECS`）：每個來源 IP 一次最多連續幾個，
    /// 之後每隔多久回補一個。預設值見 `login_rate_limit`（暫定，待需求方確認）。
    pub login_rate_burst: u32,
    pub login_rate_replenish: Duration,
    /// 每位使用者在一個滑動時間窗內最多可觸發幾次 AI 呼叫（`AI_USAGE_MAX`、`AI_USAGE_WINDOW_SECS`）。
    /// 計入使用者按下去才發生的呼叫：對話回覆、語音辨識與合成、教師翻譯總結與重新分組。
    /// 預設值見 `ai_usage`（暫定，待需求方確認）。
    pub ai_usage_max: usize,
    pub ai_usage_window: Duration,
    /// AI 服務的 API key 池（`OPENAI_API_KEYS`、`.apikeylist`）；對話與語音共用，冷卻狀態也共用。
    pub openai_keys: Arc<KeyPool>,
    /// 具體模型尚未決定（S-03.1），因此沒有預設值，必須明確設定。
    pub openai_model: String,
    /// 主模型被限流（429）時依序改用的模型；空表示不啟用備援
    pub openai_fallback_models: Vec<String>,
    /// 推理強度（`OPENAI_REASONING_EFFORT`，例如 `high`）；沒設定就不送
    pub openai_reasoning_effort: Option<String>,
    pub openai_base_url: String,
    /// 語音辨識專用的 base URL（例如自架的 Whisper）；沒設定就用 `openai_base_url`
    pub openai_stt_base_url: Option<String>,
    /// 語音合成專用的自架服務 base URL；有設定時不送 OpenAI 金鑰、不扣 AI 用量（S-05.5）。
    pub openai_tts_base_url: Option<String>,
    /// 語音備援用的 OpenAI 模型（S-05.3）。具體模型尚未決定，沒設定就不啟用語音備援。
    pub openai_stt_model: Option<String>,
    pub openai_tts_model: Option<String>,
    /// 語音合成的聲音名稱。
    pub openai_tts_voice: String,
    /// 語音對話實驗：cascade 的 `/v1/realtime` 位址；沒設定就不開放（見 `cascade`）。
    pub cascade_realtime_url: Option<String>,
    /// cascade 呼叫內部 LLM 端點用的共用密鑰。
    pub cascade_llm_token: Option<String>,
    /// 第幾回合起 AI 引導學生收尾（S-03.3）。
    pub wrap_up_turn: u32,
    /// 回合上限（S-03.3）：第幾回合時直接結束。
    pub max_turns: u32,
    /// AI 首個 token 逾時（S-03.5）。
    pub ai_first_token_timeout: Duration,
    /// AI 完整回覆上限（S-03.5）。
    pub ai_total_timeout: Duration,
    /// 一般 API 請求的逾時（#325）：從收到請求到送出回應標頭為止，含讀取 request body。
    pub request_timeout: Duration,
    /// 示範模式（`DEMO_MODE=true`）：啟動時建立固定密碼的示範帳號，並開放 `GET /api/demo/accounts`
    /// 讓登入頁一鍵填入。密碼公開，只能用在展示環境；示範帳號不含管理者。
    pub demo_mode: bool,
}

#[derive(Debug)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ConfigError {}

/// 設定來源：Workers 的 vars 與 secrets（測試時可換成 map）。
pub trait Source {
    fn get(&self, name: &str) -> Option<String>;
}

impl Source for worker::Env {
    fn get(&self, name: &str) -> Option<String> {
        self.secret(name)
            .map(|v| v.to_string())
            .or_else(|_| self.var(name).map(|v| v.to_string()))
            .ok()
    }
}

impl Source for std::collections::HashMap<String, String> {
    fn get(&self, name: &str) -> Option<String> {
        std::collections::HashMap::get(self, name).cloned()
    }
}

struct Env<'a>(&'a dyn Source);

impl Env<'_> {
    fn var(&self, name: &str) -> Result<String, ()> {
        self.0.get(name).ok_or(())
    }
}

fn required(env: &Env, name: &str) -> Result<String, ConfigError> {
    env.var(name)
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| ConfigError(format!("missing environment variable {name}")))
}

fn parse_or<T: std::str::FromStr>(env: &Env, name: &str, default: T) -> Result<T, ConfigError> {
    match env.var(name) {
        Ok(v) if !v.is_empty() => v
            .parse()
            .map_err(|_| ConfigError(format!("invalid value for {name}"))),
        _ => Ok(default),
    }
}

/// 回合設定必須是 `1 <= 引導收尾 <= 上限 <= 1000`（S-03.3）；資料庫以 `i32` 計數，
/// 同時避免 `0` 或過大的值讓對話在第一則訊息就結束。
pub fn validate_turns(wrap_up_turn: u32, max_turns: u32) -> Result<(), ConfigError> {
    if wrap_up_turn >= 1 && wrap_up_turn <= max_turns && max_turns <= 1000 {
        Ok(())
    } else {
        Err(ConfigError(
            "WRAP_UP_TURN and MAX_TURNS must satisfy 1 <= WRAP_UP_TURN <= MAX_TURNS <= 1000".into(),
        ))
    }
}

/// 以逗號分隔的信箱清單，去空白並轉小寫。
pub fn parse_admin_emails(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 逗號分隔的模型清單：去掉空白與空項；順序就是備援的先後（重複的由 `with_fallbacks` 略過）。
fn parse_model_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(String::from)
        .collect()
}

/// 讀取所有 API key：`OPENAI_API_KEYS`（逗號或換行分隔，Workers secret）。Workers 沒有檔案系統，不讀 `.apikeylist`。
fn load_openai_keys(raw: &str) -> Vec<String> {
    merge_keys(vec![parse_key_lines(&raw.replace(',', "\n"))])
}

impl Config {
    /// 回覆產生的 claim 超過多久視為殘留（秒）：兩次嘗試的上限再加一點餘裕。
    pub fn generation_stale_secs(&self) -> f64 {
        self.ai_total_timeout.as_secs_f64() * 2.0 + 10.0
    }

    /// 較長的請求逾時（#325），給同步等待 AI／語音服務或處理較久的端點：AI 完整回覆上限再加 60 秒，
    /// 容納上傳 10 MB 音訊、語音服務本身的 30 秒上限與資料庫操作；不會比一般請求逾時短。
    pub fn long_request_timeout(&self) -> Duration {
        (self.ai_total_timeout + Duration::from_secs(60)).max(self.request_timeout)
    }

    /// 伺服器端語音對話的 cascade 位址：網址與共用密鑰要同時設定才有值。
    /// 只有網址時 cascade 呼叫內部 LLM 端點會被拒絕，學生按得到即時語音卻等不到回覆。
    pub fn cascade_voice_url(&self) -> Option<&str> {
        match (&self.cascade_realtime_url, &self.cascade_llm_token) {
            (Some(url), Some(_)) => Some(url),
            _ => None,
        }
    }

    pub fn from_source(source: &dyn Source) -> Result<Self, ConfigError> {
        let env = Env(source);
        let app_base_url = required(&env, "APP_BASE_URL")?
            .trim_end_matches('/')
            .to_string();
        let wrap_up_turn = parse_or(&env, "WRAP_UP_TURN", 15)?;
        let max_turns = parse_or(&env, "MAX_TURNS", 20)?;
        validate_turns(wrap_up_turn, max_turns)?;
        let request_timeout_secs: u64 = parse_or(&env, "REQUEST_TIMEOUT_SECS", 10)?;
        if request_timeout_secs == 0 {
            return Err(ConfigError(
                "REQUEST_TIMEOUT_SECS must be at least 1".into(),
            ));
        }
        let login_rate_burst: u32 = parse_or(
            &env,
            "LOGIN_RATE_BURST",
            crate::login_rate_limit::DEFAULT_BURST,
        )?;
        let login_rate_replenish_secs: u64 = parse_or(
            &env,
            "LOGIN_RATE_REPLENISH_SECS",
            crate::login_rate_limit::DEFAULT_REPLENISH.as_secs(),
        )?;
        if login_rate_burst == 0 || login_rate_replenish_secs == 0 {
            return Err(ConfigError(
                "LOGIN_RATE_BURST and LOGIN_RATE_REPLENISH_SECS must be at least 1".into(),
            ));
        }
        let ai_usage_max: usize = parse_or(&env, "AI_USAGE_MAX", crate::ai_usage::DEFAULT_MAX)?;
        let ai_usage_window_secs: u64 = parse_or(
            &env,
            "AI_USAGE_WINDOW_SECS",
            crate::ai_usage::DEFAULT_WINDOW.as_secs(),
        )?;
        if ai_usage_max == 0 || ai_usage_window_secs == 0 {
            return Err(ConfigError(
                "AI_USAGE_MAX and AI_USAGE_WINDOW_SECS must be at least 1".into(),
            ));
        }
        Ok(Self {
            google_client_id: required(&env, "GOOGLE_CLIENT_ID")?,
            google_client_secret: required(&env, "GOOGLE_CLIENT_SECRET")?,
            google_redirect_url: env
                .var("GOOGLE_REDIRECT_URL")
                .unwrap_or_else(|_| format!("{app_base_url}/api/auth/google/callback")),
            admin_emails: parse_admin_emails(&env.var("ADMIN_EMAILS").unwrap_or_default()),
            admin_initial_password: env
                .var("ADMIN_INITIAL_PASSWORD")
                .ok()
                .filter(|v| !v.is_empty()),
            cookie_secure: env
                .var("COOKIE_SECURE")
                .map(|v| v != "false")
                .unwrap_or(true),
            trusted_proxies: crate::client_ip::parse_cidrs(
                &env.var("TRUSTED_PROXIES").unwrap_or_default(),
            )
            .map_err(|e| ConfigError(format!("invalid TRUSTED_PROXIES: {e}")))?,
            login_rate_burst,
            login_rate_replenish: Duration::from_secs(login_rate_replenish_secs),
            ai_usage_max,
            ai_usage_window: Duration::from_secs(ai_usage_window_secs),
            openai_keys: Arc::new(
                KeyPool::new(load_openai_keys(
                    &env.var("OPENAI_API_KEYS").unwrap_or_default(),
                ))
                .ok_or_else(|| {
                    ConfigError("no AI API key: set the OPENAI_API_KEYS secret".into())
                })?,
            ),
            openai_model: required(&env, "OPENAI_MODEL")?,
            openai_fallback_models: parse_model_list(
                &env.var("OPENAI_FALLBACK_MODELS").unwrap_or_default(),
            ),
            openai_reasoning_effort: env
                .var("OPENAI_REASONING_EFFORT")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            openai_base_url: env
                .var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".into()),
            openai_stt_base_url: env
                .var("OPENAI_STT_BASE_URL")
                .ok()
                .filter(|v| !v.is_empty()),
            openai_tts_base_url: env
                .var("OPENAI_TTS_BASE_URL")
                .ok()
                .filter(|v| !v.is_empty()),
            cascade_realtime_url: env
                .var("CASCADE_REALTIME_URL")
                .ok()
                .filter(|v| !v.is_empty()),
            cascade_llm_token: env.var("CASCADE_LLM_TOKEN").ok().filter(|v| !v.is_empty()),
            openai_stt_model: env.var("OPENAI_STT_MODEL").ok().filter(|v| !v.is_empty()),
            openai_tts_model: env.var("OPENAI_TTS_MODEL").ok().filter(|v| !v.is_empty()),
            openai_tts_voice: env
                .var("OPENAI_TTS_VOICE")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "alloy".into()),
            wrap_up_turn,
            max_turns,
            ai_first_token_timeout: Duration::from_secs(parse_or(
                &env,
                "AI_FIRST_TOKEN_TIMEOUT_SECS",
                15,
            )?),
            ai_total_timeout: Duration::from_secs(parse_or(&env, "AI_TOTAL_TIMEOUT_SECS", 60)?),
            request_timeout: Duration::from_secs(request_timeout_secs),
            demo_mode: env.var("DEMO_MODE").is_ok_and(|v| v == "true"),
            app_base_url,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_settings_are_validated() {
        assert!(validate_turns(15, 20).is_ok());
        assert!(validate_turns(1, 1).is_ok());
        for (wrap, max) in [(0, 20), (21, 20), (15, 0), (5, 1001), (5, u32::MAX)] {
            assert!(validate_turns(wrap, max).is_err(), "{wrap}/{max}");
        }
    }

    #[test]
    fn keys_are_split_on_commas_and_lines() {
        assert_eq!(load_openai_keys("k1, k2\nk3,k1"), ["k1", "k2", "k3"]);
    }
}
