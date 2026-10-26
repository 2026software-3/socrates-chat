//! 設定一律來自環境變數（S-08.3）；秘密不進版控、不進前端。

use std::{
    env, fs, io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use crate::key_pool::{KeyPool, merge_keys, parse_key_lines};

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
    /// 首位管理者內建登入的初始密碼（S-01.3）；沒設定就不建立內建管理者帳號，只能用 Google 登入。
    pub admin_initial_password: Option<String>,
    /// session cookie 是否加 `Secure`；本機 http 開發可設為 `false`。
    pub cookie_secure: bool,
    pub listen_addr: String,
    /// 受信任的反向代理網段（`TRUSTED_PROXIES`，逗號分隔的 CIDR）。只有 TCP 對端落在這些網段時，
    /// 才採用 `X-Forwarded-For` 作為來源 IP；預設為空，一律用 TCP 對端（S-08.2 登入限流）。
    pub trusted_proxies: Vec<crate::client_ip::Cidr>,
    /// 前端建置產物目錄（`frontend/dist`）；設定後由後端同網域提供（S-08.1）。
    pub frontend_dir: Option<std::path::PathBuf>,
    /// AI 服務的 API key 池（`OPENAI_API_KEYS`、`.apikeylist`）；對話與語音共用，冷卻狀態也共用。
    pub openai_keys: Arc<KeyPool>,
    /// 具體模型尚未決定（S-03.1），因此沒有預設值，必須明確設定。
    pub openai_model: String,
    /// 主模型被限流（429）時依序改用的模型；空表示不啟用備援
    pub openai_fallback_models: Vec<String>,
    pub openai_base_url: String,
    /// 語音備援用的 OpenAI 模型（S-05.3）。具體模型尚未決定，沒設定就不啟用語音備援。
    /// 語音辨識專用的 base URL（例如自架的 Whisper）；沒設定就用 `openai_base_url`
    pub openai_stt_base_url: Option<String>,
    pub openai_stt_model: Option<String>,
    pub openai_tts_model: Option<String>,
    /// 語音合成的聲音名稱。
    pub openai_tts_voice: String,
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

/// 金鑰清單檔的預設位置：後端通常在 `backend/` 執行，所以也找上一層（專案根目錄）。
const DEFAULT_KEY_FILES: [&str; 2] = [".apikeylist", "../.apikeylist"];

/// 逗號分隔的模型清單：去掉空白與空項；順序就是備援的先後（重複的由 `with_fallbacks` 略過）。
fn parse_model_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(String::from)
        .collect()
}

/// 讀取所有 API key：`OPENAI_API_KEYS`（逗號分隔）加上金鑰清單檔（一行一把），合併去重。
/// 清單檔由 `OPENAI_API_KEYS_FILE` 指定（指定了就必須讀得到），否則找預設位置，找不到也沒關係。
pub fn load_openai_keys() -> Result<Vec<String>, ConfigError> {
    let explicit = env::var("OPENAI_API_KEYS_FILE")
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    let candidates: Vec<PathBuf> = match &explicit {
        Some(p) => vec![p.clone()],
        None => DEFAULT_KEY_FILES.iter().map(PathBuf::from).collect(),
    };
    gather_keys(
        &env::var("OPENAI_API_KEYS").unwrap_or_default(),
        &candidates,
        explicit.is_some(),
    )
}

/// 沒有任何 key 時的啟動錯誤；舊的 `OPENAI_API_KEY` 還設著就明講它已改名，免得部署時摸不著頭緒。
fn no_key_error(legacy_variable_set: bool) -> ConfigError {
    ConfigError(if legacy_variable_set {
        "OPENAI_API_KEY is no longer used: put the key in the .apikeylist file (one per line) \
         or in OPENAI_API_KEYS"
            .into()
    } else {
        "no AI API key: set OPENAI_API_KEYS or fill the .apikeylist file".into()
    })
}

fn gather_keys(
    env_list: &str,
    candidates: &[PathBuf],
    file_required: bool,
) -> Result<Vec<String>, ConfigError> {
    let from_env: Vec<String> = env_list
        .split(',')
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .collect();
    let from_file = read_first_key_file(candidates, file_required)?;
    Ok(merge_keys(vec![from_env, from_file]))
}

fn read_first_key_file(candidates: &[PathBuf], required: bool) -> Result<Vec<String>, ConfigError> {
    for path in candidates {
        match fs::read_to_string(path) {
            Ok(raw) => return Ok(parse_key_lines(&raw)),
            Err(e) if e.kind() == io::ErrorKind::NotFound && !required => {}
            Err(e) => {
                // Docker secret 是 bind mount，沿用主機檔案的權限；容器內是非 root 使用者，
                // 檔案是 0600 就讀不到，所以把可能的原因講明
                return Err(ConfigError(format!(
                    "cannot read the API key file {} ({}); inside Docker the file must be \
                     readable by the container user, e.g. chmod 644",
                    Path::new(path).display(),
                    e.kind()
                )));
            }
        }
    }
    Ok(Vec::new())
}

impl Config {
    /// 回覆產生的 claim 超過多久視為殘留（秒）：兩次嘗試的上限再加一點餘裕。
    pub fn generation_stale_secs(&self) -> f64 {
        self.ai_total_timeout.as_secs_f64() * 2.0 + 10.0
    }

    pub fn from_env() -> Result<Self, ConfigError> {
        let app_base_url = required("APP_BASE_URL")?.trim_end_matches('/').to_string();
        let wrap_up_turn = parse_or("WRAP_UP_TURN", 15)?;
        let max_turns = parse_or("MAX_TURNS", 20)?;
        validate_turns(wrap_up_turn, max_turns)?;
        Ok(Self {
            database_url: required("DATABASE_URL")?,
            google_client_id: required("GOOGLE_CLIENT_ID")?,
            google_client_secret: required("GOOGLE_CLIENT_SECRET")?,
            google_redirect_url: env::var("GOOGLE_REDIRECT_URL")
                .unwrap_or_else(|_| format!("{app_base_url}/api/auth/google/callback")),
            admin_emails: parse_admin_emails(&env::var("ADMIN_EMAILS").unwrap_or_default()),
            admin_initial_password: env::var("ADMIN_INITIAL_PASSWORD")
                .ok()
                .filter(|v| !v.is_empty()),
            cookie_secure: env::var("COOKIE_SECURE")
                .map(|v| v != "false")
                .unwrap_or(true),
            listen_addr: env::var("LISTEN_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".into()),
            trusted_proxies: crate::client_ip::parse_cidrs(
                &env::var("TRUSTED_PROXIES").unwrap_or_default(),
            )
            .map_err(|e| ConfigError(format!("invalid TRUSTED_PROXIES: {e}")))?,
            frontend_dir: env::var("FRONTEND_DIR")
                .ok()
                .filter(|v| !v.is_empty())
                .map(Into::into),
            openai_keys: Arc::new(KeyPool::new(load_openai_keys()?).ok_or_else(|| {
                no_key_error(env::var("OPENAI_API_KEY").is_ok_and(|v| !v.is_empty()))
            })?),
            openai_model: required("OPENAI_MODEL")?,
            openai_fallback_models: parse_model_list(
                &env::var("OPENAI_FALLBACK_MODELS").unwrap_or_default(),
            ),
            openai_base_url: env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".into()),
            openai_stt_base_url: env::var("OPENAI_STT_BASE_URL")
                .ok()
                .filter(|v| !v.is_empty()),
            openai_stt_model: env::var("OPENAI_STT_MODEL").ok().filter(|v| !v.is_empty()),
            openai_tts_model: env::var("OPENAI_TTS_MODEL").ok().filter(|v| !v.is_empty()),
            openai_tts_voice: env::var("OPENAI_TTS_VOICE")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "alloy".into()),
            wrap_up_turn,
            max_turns,
            ai_first_token_timeout: Duration::from_secs(parse_or(
                "AI_FIRST_TOKEN_TIMEOUT_SECS",
                15,
            )?),
            ai_total_timeout: Duration::from_secs(parse_or("AI_TOTAL_TIMEOUT_SECS", 60)?),
            app_base_url,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str, content: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("socrates-keys-{}-{name}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".apikeylist");
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn keys_merge_env_and_file_without_duplicates() {
        let f = temp_file("merge", "# keys\nk2\nk3\n");
        let keys = gather_keys("k1, k2 ,", &[f], false).unwrap();
        assert_eq!(keys, ["k1", "k2", "k3"]);
    }

    #[test]
    fn missing_default_file_is_fine_but_explicit_one_is_required() {
        let missing = PathBuf::from("/nonexistent/.apikeylist");
        assert_eq!(
            gather_keys("k1", std::slice::from_ref(&missing), false).unwrap(),
            ["k1"]
        );
        assert!(gather_keys("k1", &[missing], true).is_err());
    }

    #[test]
    fn first_existing_candidate_wins() {
        let f = temp_file("first", "from-file\n");
        let keys = gather_keys("", &[PathBuf::from("/nonexistent/x"), f], false).unwrap();
        assert_eq!(keys, ["from-file"]);
    }

    #[test]
    fn unreadable_key_file_error_does_not_leak_contents() {
        let err = gather_keys("", &[env::temp_dir()], true).unwrap_err();
        assert!(err.to_string().contains("cannot read"));
    }

    #[test]
    fn missing_key_error_points_to_the_renamed_variable() {
        let legacy = no_key_error(true).to_string();
        assert!(legacy.contains("OPENAI_API_KEY"), "{legacy}");
        assert!(legacy.contains(".apikeylist"), "{legacy}");
        assert!(!no_key_error(false).to_string().contains("OPENAI_API_KEY "));
    }

    #[test]
    fn unreadable_key_file_error_explains_permissions() {
        let err = gather_keys("", &[env::temp_dir()], true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("chmod 644"), "{err}");
    }

    #[test]
    fn turn_settings_are_validated() {
        assert!(validate_turns(15, 20).is_ok());
        assert!(validate_turns(1, 1).is_ok());
        assert!(validate_turns(20, 20).is_ok());
        for (wrap, max) in [(0, 20), (21, 20), (15, 0), (5, 1001), (5, u32::MAX)] {
            assert!(validate_turns(wrap, max).is_err(), "{wrap}/{max}");
        }
    }

    #[test]
    fn model_list_is_split_on_commas_trimmed_and_cleaned() {
        assert_eq!(parse_model_list(""), Vec::<String>::new());
        assert_eq!(parse_model_list("  , ,"), Vec::<String>::new());
        assert_eq!(parse_model_list("a"), ["a"]);
        assert_eq!(
            parse_model_list(" google/gemma:free , nvidia/x:free,,a "),
            ["google/gemma:free", "nvidia/x:free", "a"]
        );
    }
}
