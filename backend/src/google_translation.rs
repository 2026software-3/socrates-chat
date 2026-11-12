use std::time::Duration;

use reqwest::{Client, header::HeaderValue, redirect::Policy};
use serde::Deserialize;

const ENDPOINT: &str = "https://translation.googleapis.com/language/translate/v2";
pub(crate) const TIMEOUT: Duration = Duration::from_secs(5);

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

    pub(crate) async fn translate(
        &self,
        source: &str,
        target: &str,
        text: [&str; 2],
    ) -> Option<[String; 2]> {
        self.translate_at(ENDPOINT, source, target, text).await
    }

    async fn translate_at(
        &self,
        endpoint: &str,
        source: &str,
        target: &str,
        text: [&str; 2],
    ) -> Option<[String; 2]> {
        #[derive(Deserialize)]
        struct Response {
            data: Data,
        }
        #[derive(Deserialize)]
        struct Data {
            translations: [Translation; 2],
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Translation {
            translated_text: String,
        }

        let text = text.map(|value| crate::pii::mask(value.to_owned()));
        let response: Response = self
            .http
            .post(endpoint)
            .header("x-goog-api-key", self.key.clone())
            .timeout(TIMEOUT)
            .json(&serde_json::json!({
                "q": text,
                "source": source,
                "target": target,
                "format": "text",
            }))
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?
            .json()
            .await
            .ok()?;
        let translations = response.data.translations.map(|item| item.translated_text);
        let placeholders = ["{n}", "[EMAIL]", "[NATIONAL_ID]", "[STUDENT_ID]", "[PHONE]"];
        text.iter()
            .zip(&translations)
            .all(|(original, translated)| {
                placeholders.iter().all(|placeholder| {
                    original.matches(*placeholder).count()
                        == translated.matches(*placeholder).count()
                })
            })
            .then_some(translations)
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
            Some(["Finish {n} dialogues".into(), "Contact [EMAIL]".into()])
        );
        assert_eq!(
            client.translate_at(&endpoint, "zh-TW", "es", input).await,
            None
        );
        assert_eq!(
            client
                .translate_at(&endpoint, "zh-TW", "zh-TW", input)
                .await,
            None
        );
        server.abort();
    }
}
