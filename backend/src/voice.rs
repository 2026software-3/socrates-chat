//! 語音備援（F-10；規格見 S-05.3、S-05.4、S-08.1）。
//!
//! 瀏覽器預設用 Web Speech API；瀏覽器不支援時才經這裡呼叫 OpenAI 的語音辨識與語音合成，
//! 前端不接觸 OpenAI。**不保存原始音訊**：上傳的音訊只在記憶體中轉送給 OpenAI，
//! 辨識完成或失敗後即丟棄，不寫入資料庫、磁碟或日誌（S-05.4）。
//!
//! 具體的語音模型尚未決定（S-03.4 之後），所以沒有預設值：`OPENAI_STT_MODEL`、`OPENAI_TTS_MODEL`
//! 沒設定時兩個端點都回 `503 voice_unavailable`，前端改提示用文字輸入。
//!
//! 設了 `OPENAI_TTS_BASE_URL` 時語音合成改走自架服務：不送 OpenAI 金鑰、不扣 AI 用量，
//! 改以每位使用者同時一個請求保護 GPU，並用 `X-Speech-Pacing` 告訴前端逐句送（S-05.5）。

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, FromRequest, Query, Request, State},
    http::{HeaderMap, HeaderName, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use reqwest::multipart;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{AppState, auth::RequireStudent, config::Config, error::ApiError, key_pool::KeyPool};

/// 一次辨識可上傳的音訊上限；單次發言上限 60 秒（S-05.2），這個大小綽綽有餘。
pub const MAX_AUDIO_BYTES: usize = 10 * 1024 * 1024;
/// 單次合成的文字上限，與訊息長度上限一致。
pub const MAX_SPEECH_CHARS: usize = 4000;
/// 自架語音時的回應標頭，值為 `sentence`：不合併句子，也沒有每則回覆的請求上限。
pub const SPEECH_PACING_HEADER: &str = "x-speech-pacing";

const SPEECH_BUSY: ApiError = ApiError::new(
    StatusCode::TOO_MANY_REQUESTS,
    "speech_busy",
    "語音合成忙碌中，請稍後再試",
);

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

    /// 語音合成是否由自架服務處理；是的話不計入 AI 用量。
    fn self_hosted_speech(&self) -> bool {
        false
    }
}

/// 自架語音正在合成的使用者。沒有用量上限擋著，一人同時只跑一個請求，以免佔滿 GPU。
#[derive(Default)]
pub(crate) struct SpeechSlots(Mutex<HashSet<Uuid>>);

impl SpeechSlots {
    fn acquire(self: &Arc<Self>, user: Uuid) -> Option<SpeechSlot> {
        let mut users = self.0.lock().unwrap_or_else(|e| e.into_inner());
        users.insert(user).then(|| SpeechSlot {
            slots: self.clone(),
            user,
        })
    }
}

/// 合成結束、失敗或客戶端中斷（future 被丟棄）時都會釋放。
struct SpeechSlot {
    slots: Arc<SpeechSlots>,
    user: Uuid,
}

impl Drop for SpeechSlot {
    fn drop(&mut self) {
        let mut users = self.slots.0.lock().unwrap_or_else(|e| e.into_inner());
        users.remove(&self.user);
    }
}

pub struct OpenAiVoice {
    http: reqwest::Client,
    base_url: String,
    stt_base_url: String,
    tts_base_url: Option<String>,
    keys: Arc<KeyPool>,
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
            stt_base_url: c
                .openai_stt_base_url
                .as_deref()
                .unwrap_or(&c.openai_base_url)
                .trim_end_matches('/')
                .to_string(),
            tts_base_url: c
                .openai_tts_base_url
                .as_deref()
                .map(|url| url.trim_end_matches('/').to_string()),
            keys: c.openai_keys.clone(),
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
        // 語音端點的錯誤（例如模型沒開放給這把 key）不回報給 pool，免得連累對話用的 key
        let lease = self.keys.acquire().ok_or(VoiceError::Failed)?;
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
            .post(format!("{}/audio/transcriptions", self.stt_base_url))
            .bearer_auth(&lease.key)
            .multipart(form)
            .send()
            .await
            .map_err(|_| VoiceError::Failed)?;
        if !res.status().is_success() {
            tracing::warn!(status = %res.status(), key = lease.index + 1, "speech-to-text returned an error status");
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
        // OpenAI 金鑰不能送到自架服務，所以那條路不取 key
        let (base_url, lease) = match &self.tts_base_url {
            Some(url) => (url, None),
            None => (
                &self.base_url,
                Some(self.keys.acquire().ok_or(VoiceError::Failed)?),
            ),
        };
        let mut req = self
            .http
            .post(format!("{base_url}/audio/speech"))
            .json(&json!({
                "model": model,
                "voice": self.tts_voice,
                "input": text,
                "response_format": "mp3",
            }));
        if let Some(lease) = &lease {
            req = req.bearer_auth(&lease.key);
        }
        let res = req.send().await.map_err(|_| VoiceError::Failed)?;
        if !res.status().is_success() {
            let key = lease.as_ref().map(|lease| lease.index + 1);
            tracing::warn!(status = %res.status(), key, "text-to-speech returned an error status");
            return Err(VoiceError::Failed);
        }
        res.bytes()
            .await
            .map(Vec::from)
            .map_err(|_| VoiceError::Failed)
    }

    fn self_hosted_speech(&self) -> bool {
        self.tts_base_url.is_some()
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

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct TranscribeQuery {
    language: String,
}

/// 只給 OpenAPI 描述二進位本文用。
#[derive(utoipa::ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
pub(crate) struct AudioBytes(Vec<u8>);

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct Transcript {
    text: String,
}

/// Translate Axum's body-size rejection into the API's standard JSON error format.
pub(crate) struct AudioBody(Bytes);

impl FromRequest<AppState> for AudioBody {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &AppState) -> Result<Self, Self::Rejection> {
        Bytes::from_request(req, state)
            .await
            .map(Self)
            .map_err(|rejection| {
                let status = rejection.into_response().status();
                if status == StatusCode::PAYLOAD_TOO_LARGE {
                    ApiError::new(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "payload_too_large",
                        "音訊大小不可超過 10 MB",
                    )
                } else {
                    ApiError::bad_request("invalid_audio", "無法讀取音訊內容")
                }
            })
    }
}

/// 辨識上傳的音訊（請求本文就是音訊位元組）；回傳文字，音訊不保存。
#[utoipa::path(
    post,
    path = "/api/voice/transcribe",
    tag = "voice",
    summary = "語音轉文字（請求本文是音訊，上限 10 MB）",
    params(TranscribeQuery),
    request_body(content = AudioBytes, content_type = "audio/*", description = "原始音訊位元組；Content-Type 須為 audio/*；辨識完成或失敗後立即丟棄，不會保存"),
    responses(
        (status = 200, description = "辨識文字", body = Transcript),
        (status = 400, description = "invalid_language：不支援的語言；invalid_audio：Content-Type 不是 audio/* 或音訊本文為空", body = crate::error::ErrorBody),
        (status = 413, description = "payload_too_large：音訊本文超過 10 MB", body = crate::error::ErrorBody),
        (status = 502, description = "voice_failed", body = crate::error::ErrorBody),
        (status = 503, description = "voice_unavailable", body = crate::error::ErrorBody),
        crate::error::CommonErrors
    )
)]
pub(crate) async fn transcribe(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Query(q): Query<TranscribeQuery>,
    headers: HeaderMap,
    body: AudioBody,
) -> Result<Json<Transcript>, ApiError> {
    let language = iso_language(&q.language)
        .ok_or(ApiError::bad_request("invalid_language", "不支援的語言"))?;
    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .filter(|m| m.starts_with("audio/"))
        .ok_or(ApiError::bad_request("invalid_audio", "需要音訊內容"))?;
    if body.0.is_empty() {
        return Err(ApiError::bad_request("invalid_audio", "需要音訊內容"));
    }
    // 驗證通過才扣 AI 用量；沒有得到結果就退回
    let slot = state.charge_ai(cu.user.id)?;
    let text = state
        .voice
        // 請求本體獨占時直接取得緩衝，不複製（最多 10 MB）
        .transcribe(Vec::from(body.0), mime, language)
        .await
        .map_err(|e| {
            state.refund_ai(cu.user.id, slot);
            voice_error(e)
        })?;
    Ok(Json(Transcript { text }))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct SpeechBody {
    text: String,
    language: String,
}

/// 把文字合成語音（mp3）。OpenAI 每次請求扣一筆 AI 用量；自架服務不扣，改為一人同時一個請求。
#[utoipa::path(
    post,
    path = "/api/voice/speech",
    tag = "voice",
    summary = "文字轉語音",
    request_body = SpeechBody,
    responses(
        (status = 200, description = "audio/mpeg", content_type = "audio/mpeg", body = AudioBytes,
            headers(("X-Speech-Pacing" = String, description = "只在自架語音時出現，值為 sentence：逐句請求，不合併"))),
        (status = 429, description = "ai_usage_exceeded：AI 用量已達上限；speech_busy：自架語音同一使用者已有請求進行中", body = crate::error::ErrorBody),
        (status = 503, description = "voice_unavailable", body = crate::error::ErrorBody),
        (status = 502, description = "voice_failed", body = crate::error::ErrorBody)
    )
)]
pub(crate) async fn speech(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Json(b): Json<SpeechBody>,
) -> Result<Response, ApiError> {
    let language = iso_language(&b.language)
        .ok_or(ApiError::bad_request("invalid_language", "不支援的語言"))?;
    let text = b.text.trim();
    if text.is_empty() || text.chars().count() > MAX_SPEECH_CHARS {
        return Err(ApiError::bad_request("invalid_text", "文字長度不符"));
    }
    if state.voice.self_hosted_speech() {
        let _slot = state.speech_slots.acquire(cu.user.id).ok_or(SPEECH_BUSY)?;
        let audio = state
            .voice
            .speak(text, language)
            .await
            .map_err(voice_error)?;
        return Ok((
            [
                (header::CONTENT_TYPE, "audio/mpeg"),
                (HeaderName::from_static(SPEECH_PACING_HEADER), "sentence"),
            ],
            audio,
        )
            .into_response());
    }
    let slot = state.charge_ai(cu.user.id)?;
    let audio = state.voice.speak(text, language).await.map_err(|e| {
        state.refund_ai(cu.user.id, slot);
        voice_error(e)
    })?;
    Ok(([(header::CONTENT_TYPE, "audio/mpeg")], audio).into_response())
}
