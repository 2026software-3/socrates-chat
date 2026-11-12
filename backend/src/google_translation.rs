use std::time::Duration;

use reqwest::{Client, header::HeaderValue, redirect::Policy};
use serde::Deserialize;

const ENDPOINT: &str = "https://translation.googleapis.com/language/translate/v2";
const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub(crate) struct GoogleTranslation {
    http: Client,
    key: HeaderValue,
}

impl GoogleTranslation {
    pub(crate) fn new(key: &str) -> Option<Self> {
        let mut key = HeaderValue::from_str(key.trim()).ok()?;
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
        self.translate_at(ENDPOINT, source, target, text).await
    }

    async fn translate_at<const N: usize>(
        &self,
        endpoint: &str,
        source: &str,
        target: &str,
        text: [&str; N],
    ) -> Option<[String; N]> {
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
        let response: Response = self
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
            .ok()?
            .error_for_status()
            .ok()?
            .json()
            .await
            .ok()?;
        let translations: [String; N] = response
            .data
            .translations
            .into_iter()
            .map(|item| item.translated_text)
            .collect::<Vec<_>>()
            .try_into()
            .ok()?;
        let placeholders = ["[EMAIL]", "[NATIONAL_ID]", "[STUDENT_ID]", "[PHONE]"];
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
    use axum::{Json, Router, http::HeaderMap, routing::post};
    use serde_json::{Value, json};

    #[tokio::test]
    async fn sends_only_masked_text_and_rejects_changed_placeholders() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/translate", listener.local_addr().unwrap());
        let app = Router::new().route(
            "/translate",
            post(|headers: HeaderMap, Json(body): Json<Value>| async move {
                assert_eq!(headers["x-goog-api-key"], "synthetic-key");
                assert_eq!(body["q"], json!(["完成活動", "聯絡 [EMAIL]"]));
                assert_eq!(body["source"], "zh-TW");
                assert_eq!(body["format"], "text");
                let (title, contact) = if body["target"] == "en" {
                    ("Finish activities", "Contact [EMAIL]")
                } else {
                    ("Completa actividades", "Contactar")
                };
                Json(json!({"data": {"translations": [
                    {"translatedText": title},
                    {"translatedText": contact}
                ]}}))
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = GoogleTranslation::new("synthetic-key").unwrap();
        let input = ["完成活動", "聯絡 student@example.com"];
        assert_eq!(
            client.translate_at(&endpoint, "zh-TW", "en", input).await,
            Some(["Finish activities".into(), "Contact [EMAIL]".into()])
        );
        assert_eq!(
            client.translate_at(&endpoint, "zh-TW", "es", input).await,
            None
        );
        server.abort();
    }
}
