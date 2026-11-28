//! 教師 key 路徑的 AI 轉接層（S-03.1「多供應商與教師選模型」、#594）。
//!
//! 以 genai 呼叫多家供應商；每次呼叫都用明確的目標（協定家族、端點、模型、key），
//! 不讀環境變數。系統 key 路徑的 `OpenAiProvider` 不受影響。
//!
//! 日誌規則：genai 的錯誤（含 `Display`）可能帶請求內容與回應本文，也就是學生對話原文。
//! 這一層把它轉成自己的 `AiError` 後就丟掉，不記錄、不轉印；genai 自己寫的日誌由
//! `logging::env_filter` 整個關掉。

use std::{
    fmt,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use futures_util::StreamExt;
pub use genai::adapter::AdapterKind;
use genai::{
    ModelIden, ModelSpec, ServiceTarget,
    chat::{ChatMessage, ChatOptions, ChatRequest, ChatStreamEvent, JsonSpec, Usage},
    resolver::{AuthData, Endpoint},
    webc,
};

use crate::{
    ai::{
        AiError, AiProvider, AiRequest, AiStream, ChatRole, StructuredCompletion, StructuredRequest,
    },
    diagnostics::Failure,
};

/// 一次呼叫的目標。`endpoint` 由呼叫端（供應商名單，F-40.3）提供固定端點，這一層不驗證。
#[derive(Clone)]
pub struct AiTarget {
    adapter: AdapterKind,
    endpoint: String,
    model: String,
    api_key: String,
}

impl AiTarget {
    pub fn new(
        adapter: AdapterKind,
        endpoint: impl Into<String>,
        model: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        Self {
            adapter,
            endpoint: endpoint.into(),
            model: model.into(),
            api_key: api_key.into(),
        }
    }

    fn service_target(&self) -> ServiceTarget {
        // genai 以字串接上路徑，端點必須以 `/` 結尾
        let mut endpoint = self.endpoint.clone();
        if !endpoint.ends_with('/') {
            endpoint.push('/');
        }
        ServiceTarget {
            endpoint: Endpoint::from_owned(endpoint),
            auth: AuthData::from_single(self.api_key.clone()),
            model: ModelIden::new(self.adapter, self.model.clone()),
        }
    }
}

impl fmt::Debug for AiTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AiTarget")
            .field("adapter", &self.adapter)
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

/// 供應商回報的 token 用量；`None` 代表供應商沒有回報（不估算）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input: Option<u64>,
    pub output: Option<u64>,
}

impl From<&Usage> for TokenUsage {
    fn from(usage: &Usage) -> Self {
        let count = |n: Option<i32>| n.map(|n| u64::try_from(n).unwrap_or(0));
        Self {
            input: count(usage.prompt_tokens),
            output: count(usage.completion_tokens),
        }
    }
}

/// 共用的 genai client；每次呼叫再以 `provider` 綁上目標。
#[derive(Clone)]
pub struct GenaiClient {
    inner: genai::Client,
}

impl GenaiClient {
    pub fn new() -> Result<Self, AiError> {
        genai::Client::new()
            .map(|inner| Self { inner })
            .map_err(|e| classify(&e))
    }

    pub fn provider(&self, target: AiTarget) -> GenaiProvider {
        GenaiProvider {
            client: self.inner.clone(),
            target,
            usage: Arc::default(),
        }
    }
}

/// 綁定單一目標的 `AiProvider`；一個實例對應一次呼叫（用量記在實例上）。
pub struct GenaiProvider {
    client: genai::Client,
    target: AiTarget,
    usage: Arc<Mutex<TokenUsage>>,
}

impl GenaiProvider {
    /// 最近一次呼叫的用量；沒有回報時兩欄都是 `None`。
    pub fn last_usage(&self) -> TokenUsage {
        *self.usage.lock().expect("usage lock poisoned")
    }
}

#[async_trait]
impl AiProvider for GenaiProvider {
    async fn stream_chat(&self, req: AiRequest) -> Result<AiStream, AiError> {
        let messages = req
            .messages
            .into_iter()
            .map(|turn| match turn.role {
                ChatRole::User => ChatMessage::user(turn.content),
                ChatRole::Assistant => ChatMessage::assistant(turn.content),
            })
            .collect();
        let chat_req = ChatRequest::new(messages).with_system(req.system);
        let options = ChatOptions::default().with_capture_usage(true);
        let model = self.target.model.clone();

        let response = self
            .client
            .exec_chat_stream(
                ModelSpec::Target(self.target.service_target()),
                chat_req,
                Some(&options),
            )
            .await
            .map_err(|e| classify(&e).with_model(&model))?;

        let usage = self.usage.clone();
        let stream = response.stream.filter_map(move |event| {
            let usage = usage.clone();
            let model = model.clone();
            async move {
                match event {
                    Ok(ChatStreamEvent::Chunk(chunk)) => Some(Ok(chunk.content)),
                    // 模型開始推理、還沒有正文：算作首個 token，但不帶文字（S-03.5）
                    Ok(ChatStreamEvent::ReasoningChunk(_)) => Some(Ok(String::new())),
                    Ok(ChatStreamEvent::End(end)) => {
                        if let Some(u) = end.captured_usage {
                            *usage.lock().expect("usage lock poisoned") = TokenUsage::from(&u);
                        }
                        None
                    }
                    Ok(_) => None,
                    Err(e) => Some(Err(classify(&e).with_model(&model))),
                }
            }
        });
        Ok(stream.boxed())
    }

    async fn complete_json(&self, req: StructuredRequest) -> Result<StructuredCompletion, AiError> {
        let chat_req = ChatRequest::new(vec![ChatMessage::user(req.input.to_string())])
            .with_system(req.system);
        let options = ChatOptions::default()
            .with_capture_usage(true)
            .with_response_format(JsonSpec::new(req.name, req.schema));
        let model = self.target.model.clone();

        let response = self
            .client
            .exec_chat(
                ModelSpec::Target(self.target.service_target()),
                chat_req,
                Some(&options),
            )
            .await
            .map_err(|e| classify(&e).with_model(&model))?;

        *self.usage.lock().expect("usage lock poisoned") = TokenUsage::from(&response.usage);
        let used_model = response.provider_model_iden.model_name.to_string();
        let content = response
            .into_first_text()
            .filter(|text| !text.trim().is_empty())
            .ok_or_else(|| AiError::invalid_output().with_model(&used_model))?;
        Ok(StructuredCompletion {
            content,
            model: used_model,
        })
    }
}

// ---- 錯誤轉換：只留分類與 Retry-After，其餘（含本文）一律丟掉 ----

fn classify(error: &genai::Error) -> AiError {
    if let Some(status) = error.status() {
        let body = failed_body(error);
        let class = status_class(status.as_u16(), body);
        let header_number = |name: &str| {
            error
                .headers()
                .and_then(|h| h.get(name))
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok())
        };
        let retry_after = header_number("retry-after").map(Duration::from_secs);
        // 只取 `error.code` 的字串值；本文其餘部分（含 message）不看也不留
        let code = body_json(body).and_then(|v| {
            v.pointer("/error/code")
                .and_then(|c| c.as_str())
                .map(str::to_owned)
        });
        return AiError::classified(class)
            .with_retry_after(retry_after)
            .with_http_status(status.as_u16())
            .with_rate_limit_details(
                code.as_deref(),
                header_number("x-ratelimit-remaining-requests"),
                header_number("x-ratelimit-remaining-tokens"),
            );
    }
    AiError::classified(match error {
        genai::Error::WebModelCall { webc_error, .. }
        | genai::Error::WebAdapterCall { webc_error, .. } => match webc_error {
            webc::Error::Reqwest(e) if e.is_timeout() => Failure::Timeout,
            webc::Error::Reqwest(e) if e.is_connect() => Failure::ConnectionFailed,
            // 供應商正常回應，但內容不是 JSON
            webc::Error::ResponseFailedNotJson { .. }
            | webc::Error::ResponseFailedInvalidJson { .. } => Failure::InvalidOutput,
            _ => Failure::ServiceError,
        },
        // 串流是惰性的：HTTP 錯誤與連線錯誤都包在這裡的 `BoxError` 裡
        genai::Error::WebStream { error, .. } => {
            if let Some(inner) = error.downcast_ref::<genai::Error>() {
                return classify(inner);
            }
            if is_connection_error(error.as_ref()) {
                Failure::ConnectionFailed
            } else {
                Failure::ServiceError
            }
        }
        // 供應商正常回應，但內容無法解析成預期格式
        genai::Error::NoChatResponse { .. }
        | genai::Error::ChatResponseGeneration { .. }
        | genai::Error::InvalidJsonResponseElement { .. }
        | genai::Error::StreamParse { .. }
        | genai::Error::JsonValueExt(_)
        | genai::Error::SerdeJson(_) => Failure::InvalidOutput,
        _ => Failure::ServiceError,
    })
}

/// 沿 `source()` 鏈找 io 層的連線失敗；不具名 genai 內部使用的 reqwest 版本。
fn is_connection_error(error: &(dyn std::error::Error + 'static)) -> bool {
    use std::io::ErrorKind::{ConnectionAborted, ConnectionRefused, ConnectionReset, NotConnected};
    let mut current = Some(error);
    while let Some(e) = current {
        if let Some(io) = e.downcast_ref::<std::io::Error>()
            && matches!(
                io.kind(),
                ConnectionRefused | ConnectionReset | ConnectionAborted | NotConnected
            )
        {
            return true;
        }
        current = e.source();
    }
    false
}

fn failed_body(error: &genai::Error) -> Option<&str> {
    match error {
        genai::Error::HttpError { body, .. } => Some(body),
        genai::Error::WebModelCall { webc_error, .. }
        | genai::Error::WebAdapterCall { webc_error, .. } => match webc_error {
            webc::Error::ResponseFailedStatus { body, .. } => Some(body),
            _ => None,
        },
        _ => None,
    }
}

fn status_class(status: u16, body: Option<&str>) -> Failure {
    match status {
        401 | 403 => Failure::InvalidKey,
        // Gemini 對無效或已撤銷的 key 回 400（原因 API_KEY_INVALID）
        400 if body_reason(body, "API_KEY_INVALID") => Failure::InvalidKey,
        402 => Failure::QuotaExceeded,
        408 | 504 => Failure::Timeout,
        429 if is_quota_body(body) => Failure::QuotaExceeded,
        429 => Failure::RateLimited,
        // 容量錯誤（Anthropic 529 overloaded_error）與限流同等看待：換 key，再換備援模型（S-03.1）
        529 => Failure::RateLimited,
        _ if body_error_type(body, "overloaded_error") => Failure::RateLimited,
        _ => Failure::ServiceError,
    }
}

fn body_json(body: Option<&str>) -> Option<serde_json::Value> {
    body.and_then(|b| serde_json::from_str::<serde_json::Value>(b).ok())
}

/// 只比對固定的錯誤類型代碼，本文其餘部分不看也不留。
fn body_error_type(body: Option<&str>, kind: &str) -> bool {
    body_json(body).is_some_and(|v| v.pointer("/error/type").and_then(|t| t.as_str()) == Some(kind))
}

/// Google 的錯誤細節：`error.details[].reason`。
fn body_reason(body: Option<&str>, reason: &str) -> bool {
    body_json(body).is_some_and(|v| {
        v.pointer("/error/details")
            .and_then(|d| d.as_array())
            .is_some_and(|d| {
                d.iter()
                    .any(|x| x.get("reason").and_then(|r| r.as_str()) == Some(reason))
            })
    })
}

/// 只認固定的供應商代碼 `insufficient_quota`，本文其餘部分不看也不留。
fn is_quota_body(body: Option<&str>) -> bool {
    let Some(value) = body.and_then(|b| serde_json::from_str::<serde_json::Value>(b).ok()) else {
        return false;
    };
    ["/error/code", "/error/type"]
        .iter()
        .any(|p| value.pointer(p).and_then(|v| v.as_str()) == Some("insufficient_quota"))
}

#[cfg(test)]
mod tests;
