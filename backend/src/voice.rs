//! 語音備援（F-10；規格見 S-05.3、S-05.4、S-08.1）。
//!
//! 瀏覽器預設用 Web Speech API；瀏覽器不支援時才經這裡呼叫 OpenAI 的語音辨識與語音合成，
//! 前端不接觸 OpenAI。**不保存原始音訊**：上傳的音訊只在記憶體中轉送給 OpenAI，
//! 辨識完成或失敗後即丟棄，不寫入資料庫、磁碟或日誌（S-05.4）。
//!
//! 具體的語音模型尚未決定（S-03.4 之後），所以沒有預設值：`OPENAI_STT_MODEL`、`OPENAI_TTS_MODEL`
//! 沒設定時兩個端點都回 `503 voice_unavailable`，前端改提示用文字輸入。

use async_trait::async_trait;
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use reqwest::multipart;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{AppState, auth::RequireStudent, config::Config, error::ApiError};

/// 一次辨識可上傳的音訊上限；單次發言上限 60 秒（S-05.2），這個大小綽綽有餘。
pub const MAX_AUDIO_BYTES: usize = 10 * 1024 * 1024;
/// 單次合成的文字上限，與訊息長度上限一致。
pub const MAX_SPEECH_CHARS: usize = 4000;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/voice/transcribe", post(transcribe))
        .layer(DefaultBodyLimit::max(MAX_AUDIO_BYTES))
        .route("/api/voice/speech", post(speech))
}

/// 外部語音服務錯誤；不帶內部細節。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceError {
    /// 沒有設定語音模型
    Unavailable,
    /// 外部服務失敗或逾時
    Failed,
}

#[async_trait]
pub trait VoiceProvider: Send + Sync {
    /// 辨識音訊，`language` 是 ISO 639-1 代碼（`zh`、`en`、`es`）。
    async fn transcribe(
        &self,
        audio: Vec<u8>,
        mime: &str,
        language: &str,
    ) -> Result<String, VoiceError>;

    /// 合成語音，回傳 mp3 位元組。
    async fn speak(&self, text: &str, language: &str) -> Result<Vec<u8>, VoiceError>;
}

pub struct OpenAiVoice {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    stt_model: Option<String>,
    tts_model: Option<String>,
    tts_voice: String,
}

impl OpenAiVoice {
    pub fn from_config(c: &Config) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            base_url: c.openai_base_url.trim_end_matches('/').to_string(),
            api_key: c.openai_api_key.clone(),
            stt_model: c.openai_stt_model.clone(),
            tts_model: c.openai_tts_model.clone(),
            tts_voice: c.openai_tts_voice.clone(),
        }
    }
}

/// 上傳檔名只用來讓 OpenAI 判斷格式，依瀏覽器錄音的 MIME 決定副檔名。
fn extension_for(mime: &str) -> &'static str {
    let m = mime.split(';').next().unwrap_or_default().trim();
    match m {
        "audio/mp4" | "audio/x-m4a" | "audio/aac" => "m4a",
        "audio/ogg" => "ogg",
        "audio/wav" | "audio/x-wav" => "wav",
        "audio/mpeg" => "mp3",
        _ => "webm",
    }
}

#[async_trait]
impl VoiceProvider for OpenAiVoice {
    async fn transcribe(
        &self,
        audio: Vec<u8>,
        mime: &str,
        language: &str,
    ) -> Result<String, VoiceError> {
        let model = self.stt_model.as_deref().ok_or(VoiceError::Unavailable)?;
        let part = multipart::Part::bytes(audio)
            .file_name(format!("speech.{}", extension_for(mime)))
            .mime_str(mime.split(';').next().unwrap_or("audio/webm"))
            .map_err(|_| VoiceError::Failed)?;
        let form = multipart::Form::new()
            .part("file", part)
            .text("model", model.to_string())
            .text("language", language.to_string());
        let res = self
            .http
            .post(format!("{}/audio/transcriptions", self.base_url))
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await
            .map_err(|_| VoiceError::Failed)?;
        if !res.status().is_success() {
            tracing::warn!(status = %res.status(), "speech-to-text returned an error status");
            return Err(VoiceError::Failed);
        }
        let v: serde_json::Value = res.json().await.map_err(|_| VoiceError::Failed)?;
        v["text"]
            .as_str()
            .map(|t| t.trim().to_string())
            .ok_or(VoiceError::Failed)
    }

    async fn speak(&self, text: &str, _language: &str) -> Result<Vec<u8>, VoiceError> {
        // 語言由輸入文字本身決定，OpenAI 的語音合成沒有語言參數
        let model = self.tts_model.as_deref().ok_or(VoiceError::Unavailable)?;
        let res = self
            .http
            .post(format!("{}/audio/speech", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&json!({
                "model": model,
                "voice": self.tts_voice,
                "input": text,
                "response_format": "mp3",
            }))
            .send()
            .await
            .map_err(|_| VoiceError::Failed)?;
        if !res.status().is_success() {
            tracing::warn!(status = %res.status(), "text-to-speech returned an error status");
            return Err(VoiceError::Failed);
        }
        res.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|_| VoiceError::Failed)
    }
}

fn voice_error(e: VoiceError) -> ApiError {
    match e {
        VoiceError::Unavailable => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "voice_unavailable",
            "語音服務尚未啟用",
        ),
        VoiceError::Failed => ApiError::new(
            StatusCode::BAD_GATEWAY,
            "voice_failed",
            "語音服務暫時無法使用",
        ),
    }
}

/// 對話語言（S-12.1）對應的 ISO 639-1 代碼。
fn iso_language(lang: &str) -> Option<&'static str> {
    match lang {
        "zh-TW" => Some("zh"),
        "en" => Some("en"),
        "es" => Some("es"),
        _ => None,
    }
}

#[derive(Deserialize)]
struct TranscribeQuery {
    language: String,
}

#[derive(Serialize)]
struct Transcript {
    text: String,
}

/// 辨識上傳的音訊（請求本文就是音訊位元組）；回傳文字，音訊不保存。
async fn transcribe(
    _: RequireStudent,
    State(state): State<AppState>,
    Query(q): Query<TranscribeQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Transcript>, ApiError> {
    let language = iso_language(&q.language)
        .ok_or(ApiError::bad_request("invalid_language", "不支援的語言"))?;
    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .filter(|m| m.starts_with("audio/"))
        .ok_or(ApiError::bad_request("invalid_audio", "需要音訊內容"))?;
    if body.is_empty() {
        return Err(ApiError::bad_request("invalid_audio", "需要音訊內容"));
    }
    let text = state
        .voice
        .transcribe(body.to_vec(), mime, language)
        .await
        .map_err(voice_error)?;
    Ok(Json(Transcript { text }))
}

#[derive(Deserialize)]
struct SpeechBody {
    text: String,
    language: String,
}

/// 把文字合成語音（mp3）。
async fn speech(
    _: RequireStudent,
    State(state): State<AppState>,
    Json(b): Json<SpeechBody>,
) -> Result<Response, ApiError> {
    let language = iso_language(&b.language)
        .ok_or(ApiError::bad_request("invalid_language", "不支援的語言"))?;
    let text = b.text.trim();
    if text.is_empty() || text.chars().count() > MAX_SPEECH_CHARS {
        return Err(ApiError::bad_request("invalid_text", "文字長度不符"));
    }
    let audio = state
        .voice
        .speak(text, language)
        .await
        .map_err(voice_error)?;
    Ok(([(header::CONTENT_TYPE, "audio/mpeg")], audio).into_response())
}
