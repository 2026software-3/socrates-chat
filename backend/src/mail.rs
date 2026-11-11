//! 客訴通知信（S-17.3）：SMTP 設定與寄送。寄信只用於客訴通知。
//!
//! 設定全部來自環境變數；缺 SMTP 主機、寄件人或收件信箱任何一項就視為沒啟用（`smtp` 為 `None`），
//! 客訴照常保存、寄信狀態為 `skipped`。日誌只記客訴編號與結果的類別，不記描述、收件信箱或憑證。

use std::{env, fmt};

use crate::config::ConfigError;

/// SMTP 連線加密方式（`SMTP_SECURITY`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Security {
    /// 先明文連線再升級（埠 587），預設。
    StartTls,
    /// 一開始就是 TLS（埠 465）。
    Tls,
    /// 不加密：只給本機測試與信任的內部網路，帳號密碼會明文送出。
    None,
}

impl Security {
    fn default_port(self) -> u16 {
        match self {
            Security::StartTls => 587,
            Security::Tls => 465,
            Security::None => 25,
        }
    }
}

/// 寄信設定。`Debug` 不印出密碼與收件信箱。
#[derive(Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub security: Security,
    /// 寄件人，例如 `Socrates <noreply@example.edu>`。
    pub from: String,
    /// 收件信箱（可多個）。
    pub to: Vec<String>,
}

impl fmt::Debug for SmtpConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SmtpConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("security", &self.security)
            .field("username", &self.username.as_ref().map(|_| "<set>"))
            .field("password", &self.password.as_ref().map(|_| "<set>"))
            .field("recipients", &self.to.len())
            .finish()
    }
}

fn var(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

impl SmtpConfig {
    /// 讀環境變數：`SMTP_HOST`、`SMTP_PORT`、`SMTP_SECURITY`（`starttls`／`tls`／`none`）、`SMTP_USERNAME`、
    /// `SMTP_PASSWORD`、`SMTP_FROM`、`COMPLAINT_TO`（逗號分隔）。缺主機、寄件人或收件信箱就回 `None`；
    /// 有設定但格式錯誤則回錯，讓設定錯誤在啟動時就看得到。
    pub fn from_env() -> Result<Option<Self>, ConfigError> {
        let (Some(host), Some(from), Some(to_raw)) =
            (var("SMTP_HOST"), var("SMTP_FROM"), var("COMPLAINT_TO"))
        else {
            return Ok(None);
        };
        let security = match var("SMTP_SECURITY").as_deref().map(str::to_ascii_lowercase) {
            None => Security::StartTls,
            Some(s) if s == "starttls" => Security::StartTls,
            Some(s) if s == "tls" => Security::Tls,
            Some(s) if s == "none" => Security::None,
            Some(_) => {
                return Err(ConfigError(
                    "invalid value for SMTP_SECURITY (starttls, tls or none)".to_string(),
                ));
            }
        };
        let port = match var("SMTP_PORT") {
            None => security.default_port(),
            Some(p) => p
                .parse()
                .map_err(|_| ConfigError("invalid value for SMTP_PORT".to_string()))?,
        };
        let to: Vec<String> = to_raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        let config = Self {
            host,
            port,
            username: var("SMTP_USERNAME"),
            password: var("SMTP_PASSWORD"),
            security,
            from,
            to,
        };
        config.validate()?;
        Ok(Some(config))
    }

    fn validate(&self) -> Result<(), ConfigError> {
        use lettre::message::Mailbox;
        if self.to.is_empty() {
            return Err(ConfigError("COMPLAINT_TO has no address".to_string()));
        }
        // 錯誤訊息不帶位址本身，避免把收件信箱寫進啟動日誌
        self.from
            .parse::<Mailbox>()
            .map_err(|_| ConfigError("invalid value for SMTP_FROM".to_string()))?;
        for to in &self.to {
            to.parse::<Mailbox>()
                .map_err(|_| ConfigError("invalid address in COMPLAINT_TO".to_string()))?;
        }
        if self.username.is_some() != self.password.is_some() {
            return Err(ConfigError(
                "SMTP_USERNAME and SMTP_PASSWORD must be set together".to_string(),
            ));
        }
        Ok(())
    }
}

/// 客訴信的內容。只有這些欄位進得了信：沒有學號、電子郵件與對話原文（S-17.3）。
pub struct ComplaintMail {
    pub id: uuid::Uuid,
    pub category: String,
    pub description: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub conversation_id: Option<uuid::Uuid>,
    pub submitter_name: Option<String>,
    pub submitter_role: String,
    /// 只在附了對話編號時才有（該對話所屬教室）。
    pub classroom_name: Option<String>,
}

impl ComplaintMail {
    /// 標題只有固定格式（編號與類別），不含描述。
    pub fn subject(&self) -> String {
        format!("Complaint {} ({})", self.id, self.category)
    }

    pub fn body(&self) -> String {
        let mut text = format!(
            "客訴編號：{}\n類別：{}\n提交時間（UTC）：{}\n提交者：{}（{}）\n",
            self.id,
            self.category,
            self.created_at.format("%Y-%m-%d %H:%M:%S"),
            self.submitter_name.as_deref().unwrap_or("（未設定名稱）"),
            self.submitter_role,
        );
        if let Some(conversation) = self.conversation_id {
            text.push_str(&format!("對話編號：{conversation}\n"));
        }
        if let Some(classroom) = &self.classroom_name {
            text.push_str(&format!("教室：{classroom}\n"));
        }
        text.push_str(&format!("\n描述：\n{}\n", self.description));
        text
    }
}

/// 寄信失敗的類別；日誌只記這個，不記伺服器回應（可能含收件信箱）或憑證。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendFailure {
    /// 設定的位址組不成信
    Build,
    /// 連線逾時或整體逾時
    Timeout,
    /// 連線、加密、認證或伺服器拒絕
    Transport,
}

impl SendFailure {
    pub fn class(self) -> &'static str {
        match self {
            SendFailure::Build => "build",
            SendFailure::Timeout => "timeout",
            SendFailure::Transport => "transport",
        }
    }
}

/// 整封信從連線到送完的上限，避免背景工作卡在壞掉的 SMTP 伺服器上。
const SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub async fn send(config: &SmtpConfig, mail: &ComplaintMail) -> Result<(), SendFailure> {
    use lettre::{
        AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
        message::{
            Body,
            header::{ContentTransferEncoding, ContentType},
        },
        transport::smtp::authentication::Credentials,
    };

    let build = || -> Option<Message> {
        let mut builder = Message::builder()
            .from(config.from.parse().ok()?)
            .subject(mail.subject());
        for to in &config.to {
            builder = builder.to(to.parse().ok()?);
        }
        // 內文固定用 base64 傳輸編碼：中文內文在任何伺服器都不會被 8bit 處理弄壞
        let body = Body::new_with_encoding(mail.body(), ContentTransferEncoding::Base64).ok()?;
        builder.header(ContentType::TEXT_PLAIN).body(body).ok()
    };
    let message = build().ok_or(SendFailure::Build)?;

    let builder = match config.security {
        Security::StartTls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
            .map_err(|_| SendFailure::Transport)?,
        Security::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host)
            .map_err(|_| SendFailure::Transport)?,
        Security::None => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.host),
    };
    let mut builder = builder.port(config.port).timeout(Some(SEND_TIMEOUT / 2));
    if let (Some(user), Some(password)) = (&config.username, &config.password) {
        builder = builder.credentials(Credentials::new(user.clone(), password.clone()));
    }
    let transport = builder.build();
    match tokio::time::timeout(SEND_TIMEOUT, transport.send(message)).await {
        Err(_) => Err(SendFailure::Timeout),
        Ok(Err(e)) if e.is_timeout() => Err(SendFailure::Timeout),
        Ok(Err(_)) => Err(SendFailure::Transport),
        Ok(Ok(_)) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> SmtpConfig {
        SmtpConfig {
            host: "smtp.example.test".to_string(),
            port: 587,
            username: Some("mailer-user".to_string()),
            password: Some("p4ssw0rd-secret".to_string()),
            security: Security::StartTls,
            from: "Socrates <noreply@example.test>".to_string(),
            to: vec!["ops@example.test".to_string()],
        }
    }

    #[test]
    fn debug_output_has_no_credentials_or_recipients() {
        let text = format!("{:?}", config());
        for secret in [
            "mailer-user",
            "p4ssw0rd-secret",
            "ops@example.test",
            "noreply@example.test",
        ] {
            assert!(!text.contains(secret), "{secret} 出現在 Debug 輸出：{text}");
        }
        assert!(text.contains("smtp.example.test"));
    }

    #[test]
    fn a_valid_config_passes_and_bad_ones_are_rejected() {
        assert!(config().validate().is_ok());

        let mut c = config();
        c.to = vec![];
        assert!(c.validate().is_err(), "沒有收件信箱");

        let mut c = config();
        c.to = vec!["not an address".to_string()];
        let err = c.validate().unwrap_err().to_string();
        assert!(!err.contains("not an address"), "錯誤訊息不帶位址：{err}");

        let mut c = config();
        c.from = "nope".to_string();
        assert!(c.validate().is_err(), "寄件人格式錯誤");

        let mut c = config();
        c.password = None;
        assert!(c.validate().is_err(), "帳號與密碼要一起設定");
    }

    #[test]
    fn the_mail_subject_has_no_description_and_the_body_only_the_allowed_fields() {
        let mail = ComplaintMail {
            id: uuid::Uuid::nil(),
            category: "voice".to_string(),
            description: "語音斷線".to_string(),
            created_at: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            conversation_id: None,
            submitter_name: None,
            submitter_role: "student".to_string(),
            classroom_name: None,
        };
        assert_eq!(
            mail.subject(),
            "Complaint 00000000-0000-0000-0000-000000000000 (voice)"
        );
        let body = mail.body();
        assert!(body.contains("語音斷線") && body.contains("（student）"));
        assert!(!body.contains("對話編號") && !body.contains("教室"));
    }
}
