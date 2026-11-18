//! 開放給教師的 AI 供應商名單與驗證探測（F-40.3，#595；S-03.1「多供應商與教師選模型」）。
//!
//! 名單寫死在程式裡，不跟著 genai 升級增加，也不開放自訂 base URL：每家只用「單一 API key＋固定 https 端點」。
//! 排除 ollama 與 ollama_cloud、omlx（本機端點，SSRF）、vertex、bedrock、github_copilot、opencode_go（不是單一 key），
//! `openai_resp` 併入 OpenAI（名單只用 Chat Completions）。cohere 走自己的協定，這一版不納入。
//!
//! 兩種探測都有逾時，也不記錄 key 或回應內容：`verify_key` 只呼叫模型清單端點、不送對話內容；
//! `verify_model` 送一次最小的串流與一次 JSON 結構化輸出請求（內容是固定的探測字串）。

use std::time::Duration;

use serde_json::json;
use tokio_stream::StreamExt;

use crate::{
    ai::{AiProvider, AiRequest, ChatRole, ChatTurn, StructuredRequest},
    diagnostics::Failure,
    genai_provider::{AdapterKind, AiTarget, GenaiClient},
};

#[derive(Debug, Clone, Copy)]
pub struct Provider {
    /// 識別碼：小寫英數、底線、連字號（與資料庫的格式限制一致）。
    pub id: &'static str,
    pub name: &'static str,
    pub adapter: AdapterKind,
    /// 固定的公開 https 端點，以 `/` 結尾。
    pub endpoint: &'static str,
}

const PROVIDERS: [Provider; 9] = [
    Provider {
        id: "openai",
        name: "OpenAI",
        adapter: AdapterKind::OpenAI,
        endpoint: "https://api.openai.com/v1/",
    },
    Provider {
        id: "anthropic",
        name: "Anthropic",
        adapter: AdapterKind::Anthropic,
        endpoint: "https://api.anthropic.com/v1/",
    },
    Provider {
        id: "gemini",
        name: "Google Gemini",
        adapter: AdapterKind::Gemini,
        endpoint: "https://generativelanguage.googleapis.com/v1beta/",
    },
    Provider {
        id: "groq",
        name: "Groq",
        adapter: AdapterKind::Groq,
        endpoint: "https://api.groq.com/openai/v1/",
    },
    Provider {
        id: "deepseek",
        name: "DeepSeek",
        adapter: AdapterKind::DeepSeek,
        endpoint: "https://api.deepseek.com/v1/",
    },
    Provider {
        id: "xai",
        name: "xAI",
        adapter: AdapterKind::Xai,
        endpoint: "https://api.x.ai/v1/",
    },
    Provider {
        id: "mistral",
        name: "Mistral AI",
        adapter: AdapterKind::Mistral,
        endpoint: "https://api.mistral.ai/v1/",
    },
    Provider {
        id: "together",
        name: "Together AI",
        adapter: AdapterKind::Together,
        endpoint: "https://api.together.xyz/v1/",
    },
    Provider {
        id: "fireworks",
        name: "Fireworks AI",
        adapter: AdapterKind::Fireworks,
        endpoint: "https://api.fireworks.ai/inference/v1/",
    },
];

pub fn providers() -> &'static [Provider] {
    &PROVIDERS
}

/// 依識別碼精確查找（區分大小寫）。
pub fn find(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// 探測的逾時：key 認證測試整體、以及模型驗證的每一步。
pub const KEY_CHECK_TIMEOUT: Duration = Duration::from_secs(10);
pub const MODEL_STEP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCheck {
    Usable,
    ConnectionFailed,
}

/// key 認證測試：只呼叫模型清單端點。成功、額度不足（402）與頻率限制（429）都算可用（key 本身有效）；
/// key 無效或已撤銷、其他錯誤、逾時都算連線失敗。
pub async fn verify_key(provider: &Provider, key: &str) -> KeyCheck {
    verify_key_at(provider.endpoint, provider.adapter, key, KEY_CHECK_TIMEOUT).await
}

/// 讓路由可以在測試中換掉真正的對外探測。
#[async_trait::async_trait]
pub trait KeyVerifier: Send + Sync {
    async fn verify_key(&self, provider: &Provider, key: &str) -> KeyCheck;
}

/// 實際呼叫供應商模型清單端點的探測。
pub struct HttpKeyVerifier;

#[async_trait::async_trait]
impl KeyVerifier for HttpKeyVerifier {
    async fn verify_key(&self, provider: &Provider, key: &str) -> KeyCheck {
        verify_key(provider, key).await
    }
}

/// 讓路由可以在測試中換掉真正的模型驗證（會對供應商送出少量請求）。
#[async_trait::async_trait]
pub trait ModelVerifier: Send + Sync {
    async fn verify_model(
        &self,
        provider: &Provider,
        model: &str,
        key: &str,
    ) -> Result<(), ModelCheckError>;
}

pub struct HttpModelVerifier;

#[async_trait::async_trait]
impl ModelVerifier for HttpModelVerifier {
    async fn verify_model(
        &self,
        provider: &Provider,
        model: &str,
        key: &str,
    ) -> Result<(), ModelCheckError> {
        verify_model(provider, model, key).await
    }
}

async fn verify_key_at(
    endpoint: &str,
    adapter: AdapterKind,
    key: &str,
    timeout: Duration,
) -> KeyCheck {
    let Ok(client) = reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
    else {
        return KeyCheck::ConnectionFailed;
    };
    let mut request = client.get(format!("{endpoint}models"));
    // 各家文件上的認證方式；key 只放標頭，不放網址
    request = match adapter {
        AdapterKind::Anthropic => request
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01"),
        AdapterKind::Gemini => request.header("x-goog-api-key", key),
        _ => request.bearer_auth(key),
    };
    match request.send().await {
        Ok(response) => {
            let status = response.status();
            if status.is_success() || matches!(status.as_u16(), 402 | 429) {
                KeyCheck::Usable
            } else {
                KeyCheck::ConnectionFailed
            }
        }
        Err(_) => KeyCheck::ConnectionFailed,
    }
}

/// 模型驗證失敗的原因碼；前端再翻成文字。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelCheckError {
    AuthFailed,
    ModelNotFound,
    StreamingUnsupported,
    StructuredOutputUnsupported,
    Other,
}

impl ModelCheckError {
    pub fn code(self) -> &'static str {
        match self {
            Self::AuthFailed => "auth_failed",
            Self::ModelNotFound => "model_not_found",
            Self::StreamingUnsupported => "streaming_unsupported",
            Self::StructuredOutputUnsupported => "structured_output_unsupported",
            Self::Other => "other",
        }
    }
}

/// 模型驗證：對（供應商、模型、key）送一次最小的串流請求與一次 JSON 結構化輸出請求，兩者都成功才通過。
pub async fn verify_model(
    provider: &Provider,
    model: &str,
    key: &str,
) -> Result<(), ModelCheckError> {
    verify_model_at(
        provider.endpoint,
        provider.adapter,
        model,
        key,
        MODEL_STEP_TIMEOUT,
    )
    .await
}

/// 區分原因靠 HTTP 狀態碼與錯誤分類；各家細節以假服務測試固定，實際供應商的行為待實測（PR 標示）。
async fn verify_model_at(
    endpoint: &str,
    adapter: AdapterKind,
    model: &str,
    key: &str,
    step_timeout: Duration,
) -> Result<(), ModelCheckError> {
    let client = GenaiClient::new().map_err(|_| ModelCheckError::Other)?;
    let provider = client.provider(AiTarget::new(adapter, endpoint, model, key));

    // 串流
    let stream_step = async {
        let mut stream = provider
            .stream_chat(AiRequest {
                system: "Reply with the single word OK.".into(),
                messages: vec![ChatTurn {
                    role: ChatRole::User,
                    content: "ping".into(),
                }],
            })
            .await?;
        let mut got_text = false;
        while let Some(item) = stream.next().await {
            if !item?.is_empty() {
                got_text = true;
            }
        }
        Ok::<_, crate::ai::AiError>(got_text)
    };
    match tokio::time::timeout(step_timeout, stream_step).await {
        Err(_) => return Err(ModelCheckError::Other),
        Ok(Ok(true)) => {}
        // 串流結束卻沒有任何文字：視為不支援串流
        Ok(Ok(false)) => return Err(ModelCheckError::StreamingUnsupported),
        Ok(Err(e)) => {
            return Err(match (e.class, e.http_status) {
                (Failure::InvalidKey, _) => ModelCheckError::AuthFailed,
                (_, Some(404)) => ModelCheckError::ModelNotFound,
                // 供應商回 200 卻不是事件串流
                (Failure::InvalidOutput, _) => ModelCheckError::StreamingUnsupported,
                _ => ModelCheckError::Other,
            });
        }
    }

    // JSON 結構化輸出
    let json_step = provider.complete_json(StructuredRequest {
        system: "Return the JSON object {\"ok\": true}.".into(),
        input: json!({"probe": "ping"}),
        schema: json!({
            "type": "object",
            "properties": {"ok": {"type": "boolean"}},
            "required": ["ok"],
            "additionalProperties": false
        }),
        name: "probe".into(),
    });
    match tokio::time::timeout(step_timeout, json_step).await {
        Err(_) => Err(ModelCheckError::Other),
        Ok(Ok(done)) => {
            if serde_json::from_str::<serde_json::Value>(&done.content).is_ok_and(|v| v.is_object())
            {
                Ok(())
            } else {
                Err(ModelCheckError::StructuredOutputUnsupported)
            }
        }
        Ok(Err(e)) => Err(match (e.class, e.http_status) {
            (Failure::InvalidKey, _) => ModelCheckError::AuthFailed,
            (_, Some(404)) => ModelCheckError::ModelNotFound,
            // 400 多半是參數不被接受（例如 response_format）
            (_, Some(400)) | (Failure::InvalidOutput, _) => {
                ModelCheckError::StructuredOutputUnsupported
            }
            _ => ModelCheckError::Other,
        }),
    }
}

#[cfg(test)]
mod tests;
