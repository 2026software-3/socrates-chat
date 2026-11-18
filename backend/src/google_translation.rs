use std::time::Duration;

use reqwest::{Client, StatusCode, header::HeaderValue, redirect::Policy};
use serde::Deserialize;

const ENDPOINT: &str = "https://translation.googleapis.com/language/translate/v2";
pub(crate) const TIMEOUT: Duration = Duration::from_secs(5);
/// Google 403 錯誤的 `errors[].reason`（機器可讀，優先比對）。
const QUOTA_REASONS: [&str; 2] = ["dailyLimitExceeded", "userRateLimitExceeded"];
/// 同一 403 的 `error.message`（人類可讀，作為後備比對）。
const QUOTA_MESSAGES: [&str; 2] = ["Daily Limit Exceeded", "User Rate Limit Exceeded"];

/// 翻譯失敗的種類。配額錯誤與其他失敗都會回退到 AI，只有配額錯誤會留下紀錄。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// 每日或每分鐘配額用盡（HTTP 403）。
    Quota,
    /// 其他失敗：網路、逾時、非 403 的錯誤、回應格式錯誤或佔位符不一致。
    Other,
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: ApiError,
}

#[derive(Deserialize)]
struct ApiError {
    #[serde(default)]
    message: String,
    #[serde(default)]
    errors: Vec<ApiErrorItem>,
}

#[derive(Deserialize)]
struct ApiErrorItem {
    #[serde(default)]
    reason: String,
}

/// 只有 403 且符合配額的 reason 或 message 才算配額錯誤；回應不是預期 JSON 時歸為其他失敗。
fn classify_forbidden(body: &str) -> Failure {
    let Ok(parsed) = serde_json::from_str::<ErrorResponse>(body) else {
        return Failure::Other;
    };
    let quota = QUOTA_MESSAGES.contains(&parsed.error.message.as_str())
        || parsed
            .error
            .errors
            .iter()
            .any(|item| QUOTA_REASONS.contains(&item.reason.as_str()));
    if quota {
        Failure::Quota
    } else {
        Failure::Other
    }
}

#[derive(Clone)]
pub(crate) struct GoogleTranslation {
    http: Client,
    key: HeaderValue,
}

impl GoogleTranslation {
    pub(crate) fn new(key: &str) -> Option<Self> {
        let key = key.trim();
        if key.is_empty() {
            return None;
        }
        let mut key = HeaderValue::from_str(key).ok()?;
        key.set_sensitive(true);
        Some(Self {
            http: Client::builder().redirect(Policy::none()).build().ok()?,
            key,
        })
    }

    pub(crate) async fn translate<const N: usize>(
        &self,
        source: &str,
        target: &str,
        text: [&str; N],
    ) -> Option<[String; N]> {
        match self.translate_at(ENDPOINT, source, target, text).await {
            Ok(translated) => Some(translated),
            Err(Failure::Quota) => {
                tracing::warn!("google translation quota exceeded; falling back to AI");
                None
            }
            Err(Failure::Other) => None,
        }
    }

    async fn translate_at<const N: usize>(
        &self,
        endpoint: &str,
        source: &str,
        target: &str,
        text: [&str; N],
    ) -> Result<[String; N], Failure> {
        #[derive(Deserialize)]
        struct Response {
            data: Data,
        }
        #[derive(Deserialize)]
        struct Data {
            translations: Vec<Translation>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Translation {
            translated_text: String,
        }

        let text = text.map(|value| crate::pii::mask(value.to_owned()));
        let response = self
            .http
            .post(endpoint)
            .header("x-goog-api-key", self.key.clone())
            .timeout(TIMEOUT)
            .json(&serde_json::json!({
                "q": text.as_slice(),
                "source": source,
                "target": target,
                "format": "text",
            }))
            .send()
            .await
            .map_err(|_| Failure::Other)?;
        if response.status() == StatusCode::FORBIDDEN {
            let body = response.text().await.unwrap_or_default();
            return Err(classify_forbidden(&body));
        }
        let response: Response = response
            .error_for_status()
            .map_err(|_| Failure::Other)?
            .json()
            .await
            .map_err(|_| Failure::Other)?;
        let translations: [String; N] = response
            .data
            .translations
            .into_iter()
            .map(|item| item.translated_text)
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| Failure::Other)?;
        let placeholders = ["{n}", "[EMAIL]", "[NATIONAL_ID]", "[STUDENT_ID]", "[PHONE]"];
        let placeholders_match = text
            .iter()
            .zip(&translations)
            .all(|(original, translated)| {
                placeholders.iter().all(|placeholder| {
                    original.matches(*placeholder).count()
                        == translated.matches(*placeholder).count()
                })
            });
        if placeholders_match {
            Ok(translations)
        } else {
            Err(Failure::Other)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::post,
    };
    use serde_json::{Value, json};

    #[tokio::test]
    async fn sends_only_masked_text_and_rejects_changed_placeholders() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/translate", listener.local_addr().unwrap());
        let app = Router::new().route(
            "/translate",
            post(|headers: HeaderMap, Json(body): Json<Value>| async move {
                assert_eq!(headers["x-goog-api-key"], "synthetic-key");
                assert_eq!(
                    body,
                    json!({
                        "q": ["完成 {n} 場", "聯絡 [EMAIL]"],
                        "source": "zh-TW",
                        "target": body["target"],
                        "format": "text",
                    })
                );
                if body["target"] == "zh-TW" {
                    return StatusCode::SERVICE_UNAVAILABLE.into_response();
                }
                let title = if body["target"] == "en" {
                    "Finish {n} dialogues"
                } else {
                    "Completa diálogos"
                };
                Json(json!({"data": {"translations": [
                    {"translatedText": title},
                    {"translatedText": "Contact [EMAIL]"}
                ]}}))
                .into_response()
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = GoogleTranslation::new("synthetic-key").unwrap();
        let input = ["完成 {n} 場", "聯絡 student@example.com"];
        assert_eq!(
            client.translate_at(&endpoint, "zh-TW", "en", input).await,
            Ok(["Finish {n} dialogues".into(), "Contact [EMAIL]".into()])
        );
        assert_eq!(
            client.translate_at(&endpoint, "zh-TW", "es", input).await,
            Err(Failure::Other)
        );
        assert_eq!(
            client
                .translate_at(&endpoint, "zh-TW", "zh-TW", input)
                .await,
            Err(Failure::Other)
        );
        server.abort();
    }

    #[tokio::test]
    async fn classifies_quota_403_separately_from_other_failures() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/translate", listener.local_addr().unwrap());
        let app = Router::new().route(
            "/translate",
            post(|Json(body): Json<Value>| async move {
                match body["target"].as_str().unwrap_or_default() {
                    "en" => (
                        StatusCode::FORBIDDEN,
                        Json(json!({"error": {
                            "code": 403,
                            "message": "Daily Limit Exceeded",
                            "errors": [{
                                "message": "Daily Limit Exceeded",
                                "domain": "usageLimits",
                                "reason": "dailyLimitExceeded",
                            }],
                            "status": "PERMISSION_DENIED",
                        }})),
                    )
                        .into_response(),
                    "es" => (
                        StatusCode::FORBIDDEN,
                        Json(json!({"error": {
                            "code": 403,
                            "message": "User Rate Limit Exceeded",
                            "errors": [{"reason": "userRateLimitExceeded"}],
                        }})),
                    )
                        .into_response(),
                    "ja" => (
                        StatusCode::FORBIDDEN,
                        Json(json!({"error": {
                            "code": 403,
                            "message": "The caller does not have permission",
                            "errors": [{"reason": "forbidden"}],
                        }})),
                    )
                        .into_response(),
                    _ => (StatusCode::FORBIDDEN, "blocked by proxy").into_response(),
                }
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = GoogleTranslation::new("synthetic-key").unwrap();
        let input = ["完成 {n} 場"];
        assert_eq!(
            client.translate_at(&endpoint, "zh-TW", "en", input).await,
            Err(Failure::Quota)
        );
        assert_eq!(
            client.translate_at(&endpoint, "zh-TW", "es", input).await,
            Err(Failure::Quota)
        );
        assert_eq!(
            client.translate_at(&endpoint, "zh-TW", "ja", input).await,
            Err(Failure::Other)
        );
        assert_eq!(
            client
                .translate_at(&endpoint, "zh-TW", "zh-TW", input)
                .await,
            Err(Failure::Other)
        );
        server.abort();
    }
}
