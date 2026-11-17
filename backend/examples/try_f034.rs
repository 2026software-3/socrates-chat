use std::{env, error::Error, sync::Arc, time::Duration};

use serde::Deserialize;
use serde_json::{Map, json};
use socrates_chat_backend::{
    ai::{OpenAiProvider, collect_background},
    config::load_openai_keys,
    key_pool::KeyPool,
    pii::MaskingAi,
    topic_dimensions::{
        PROMPT_VERSION, build_request, build_translation_request, parse_generation,
        parse_translation, translation_text,
    },
};

const CASES: &str = include_str!("../eval/f03_4_cases.json");
const REVIEW_VERSION: &str = "f03.4-review-v1";

#[derive(Deserialize)]
struct Case {
    id: String,
    language: String,
    title: String,
    description: String,
    coverage: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let model = env::var("OPENAI_MODEL")?;
    let url = env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".into());
    let keys = load_openai_keys().map_err(|_| "OpenAI credentials are unavailable")?;
    let keys = Arc::new(KeyPool::new(keys).ok_or("OpenAI credentials are unavailable")?);
    let provider = MaskingAi::new(Arc::new(OpenAiProvider::new(&url, keys, &model)));
    let limit = Duration::from_secs(90);
    let cases: Vec<Case> = serde_json::from_str(CASES)?;
    for case in cases {
        let raw = collect_background(
            &provider,
            build_request(&case.title, &case.description, &case.language),
            limit,
        )
        .await
        .ok();
        let generated = raw.as_deref().and_then(parse_generation);
        let mut translations = Map::new();
        if let Some(dimensions) = &generated {
            for language in ["zh-TW", "en", "es"] {
                if language != case.language {
                    let request = build_translation_request(
                        &case.language,
                        language,
                        translation_text(dimensions),
                    );
                    let response = collect_background(&provider, request, limit).await.ok();
                    translations.insert(
                        language.to_owned(),
                        json!(response.as_deref().and_then(parse_translation)),
                    );
                }
            }
        }
        println!(
            "{}",
            json!({
                "case": case.id, "source_language": case.language, "coverage": case.coverage,
                "model": model, "prompt_version": PROMPT_VERSION, "review_version": REVIEW_VERSION,
                "valid_generation": generated.is_some(), "dimensions": generated,
                "translations": translations, "qualitative_review": "pending",
            })
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_all_source_languages_with_versioned_generation_and_translation_prompts() {
        let cases: Vec<Case> = serde_json::from_str(CASES).unwrap();
        assert_eq!(
            cases
                .iter()
                .map(|case| case.language.as_str())
                .collect::<Vec<_>>(),
            ["zh-TW", "en", "es"]
        );
        for case in cases {
            assert!(case.coverage.len() >= 4);
            let request = build_request(&case.title, &case.description, &case.language);
            assert!(request.system.contains(PROMPT_VERSION));
            assert!(request.system.contains("Avoid synonyms"));
            assert!(request.system.contains("Score 0"));
            let input: serde_json::Value =
                serde_json::from_str(&request.messages[0].content).unwrap();
            assert_eq!(input["title"], case.title);
        }
        assert!(parse_generation("[]").is_none());
        assert!(parse_translation(r#"["too short"]"#).is_none());
    }
}
