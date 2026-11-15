//! F-24.1-AI (#521): fixed-scenario evaluation of single-conversation radar scoring.
//! Run from backend/:
//!   cargo test --example try_f24_1   offline checks of the scenario set, no API calls
//!   cargo run --example try_f24_1    real model; needs OPENAI_MODEL and keys, uses API quota
//! Input: backend/eval/f24_1_cases.json (synthetic conversations only).
//! Output: one JSON record per scenario on stdout, then a summary record.

use std::{collections::BTreeMap, env, error::Error, sync::Arc, time::Duration};

use serde::Deserialize;
use serde_json::json;
use socrates_chat_backend::{
    ai::{AiRequest, ChatRole, ChatTurn, OpenAiProvider},
    config::load_openai_keys,
    key_pool::KeyPool,
    pii::MaskingAi,
    radar_scoring::{Dimension, PROMPT_VERSION, build_request, rubric_for, score},
    radar_store::{Rubric, Scores},
};

const CASES: &str = include_str!("../eval/f24_1_cases.json");
const EVALUATOR_VERSION: &str = "f24.1-eval-v1";
/// S-04.2: scoring the same conversation again may move each dimension by at most 1.
const MAX_SPREAD: u8 = 1;

#[derive(Deserialize)]
struct Suite {
    topics: BTreeMap<String, Topic>,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Topic {
    title: String,
    description: String,
    dimensions: Vec<Dimension>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    topic: String,
    language: String,
    covers: Vec<String>,
    turns: Vec<Turn>,
    /// Dimension id to the inclusive `[min, max]` a reviewer expects; unlisted ones are open.
    expected: BTreeMap<String, [u8; 2]>,
}

#[derive(Deserialize)]
struct Turn {
    role: String,
    content: String,
}

fn request(suite: &Suite, case: &Case) -> (AiRequest, Rubric) {
    let topic = &suite.topics[&case.topic];
    let turns: Vec<ChatTurn> = case
        .turns
        .iter()
        .map(|turn| ChatTurn {
            role: ChatRole::from_db_role(&turn.role),
            content: turn.content.clone(),
        })
        .collect();
    (
        build_request(&topic.title, &topic.description, &topic.dimensions, &turns),
        rubric_for(&topic.dimensions),
    )
}

/// Highest minus lowest score per dimension across the valid runs.
fn spread(runs: &[Scores]) -> BTreeMap<String, u8> {
    let mut range: BTreeMap<String, (u8, u8)> = BTreeMap::new();
    for scores in runs {
        for (id, score) in scores {
            let entry = range.entry(id.clone()).or_insert((*score, *score));
            *entry = (entry.0.min(*score), entry.1.max(*score));
        }
    }
    range
        .into_iter()
        .map(|(id, (low, high))| (id, high - low))
        .collect()
}

/// Dimensions where any valid run fell outside the reviewer's expected range.
fn outside_expected(expected: &BTreeMap<String, [u8; 2]>, runs: &[Scores]) -> Vec<String> {
    expected
        .iter()
        .filter(|(id, [low, high])| {
            runs.iter().any(|scores| {
                scores
                    .get(*id)
                    .is_none_or(|score| score < low || score > high)
            })
        })
        .map(|(id, _)| id.clone())
        .collect()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let model = env::var("OPENAI_MODEL")?;
    let url = env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".into());
    let keys = load_openai_keys().map_err(|_| "OpenAI credentials are unavailable")?;
    let keys = Arc::new(KeyPool::new(keys).ok_or("OpenAI credentials are unavailable")?);
    let provider = MaskingAi::new(Arc::new(OpenAiProvider::new(&url, keys, &model)));
    let repeats: usize = env::var("EVAL_REPEATS")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|n| *n >= 2)
        .unwrap_or(3);
    let limit = Duration::from_secs(90);
    let only = env::var("CASE").ok();
    let suite: Suite = serde_json::from_str(CASES)?;
    let (mut stable_cases, mut in_range_cases, mut total) = (0, 0, 0);
    for case in &suite.cases {
        if only.as_ref().is_some_and(|id| id != &case.id) {
            continue;
        }
        total += 1;
        let (request, rubric) = request(&suite, case);
        let mut runs = Vec::new();
        for _ in 0..repeats {
            if let Ok(scores) = score(&provider, request.clone(), &rubric, limit).await {
                runs.push(scores);
            }
        }
        let spread = spread(&runs);
        let complete = runs.len() == repeats;
        let stable = complete && spread.values().all(|gap| *gap <= MAX_SPREAD);
        let outside = outside_expected(&case.expected, &runs);
        let in_range = complete && outside.is_empty();
        stable_cases += usize::from(stable);
        in_range_cases += usize::from(in_range);
        println!(
            "{}",
            json!({
                "case": case.id, "covers": case.covers, "language": case.language,
                "model": model, "prompt_version": PROMPT_VERSION,
                "evaluator_version": EVALUATOR_VERSION, "repeats": repeats,
                "refused_or_failed_runs": repeats - runs.len(), "runs": runs,
                "spread": spread, "stable": stable,
                "expected": case.expected, "outside_expected": outside, "in_range": in_range,
                "qualitative_review": "pending",
            })
        );
    }
    println!(
        "{}",
        json!({
            "summary": true, "model": model, "prompt_version": PROMPT_VERSION,
            "evaluator_version": EVALUATOR_VERSION, "repeats": repeats, "cases": total,
            "stable_cases": stable_cases, "in_range_cases": in_range_cases,
        })
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suite() -> Suite {
        serde_json::from_str(CASES).unwrap()
    }

    #[test]
    fn scenarios_cover_the_required_situations_with_complete_topics() {
        let suite = suite();
        for required in [
            "insufficient_evidence",
            "revised_position",
            "multilingual",
            "isolation_pair",
            "facilitator_is_not_evidence",
        ] {
            assert!(
                suite
                    .cases
                    .iter()
                    .any(|case| case.covers.iter().any(|cover| cover == required)),
                "{required}"
            );
        }
        for topic in suite.topics.values() {
            let ids: Vec<&str> = topic.dimensions.iter().map(|d| d.id.as_str()).collect();
            assert_eq!(ids, ["d1", "d2", "d3", "d4", "d5", "d6"]);
        }
        for case in &suite.cases {
            let topic = &suite.topics[&case.topic];
            assert!(case.turns.iter().any(|turn| turn.role == "student"));
            assert!(["zh-TW", "en", "es"].contains(&case.language.as_str()));
            for (id, [low, high]) in &case.expected {
                assert!(topic.dimensions.iter().any(|d| &d.id == id), "{id}");
                assert!(low <= high && *high <= 5, "{} {id}", case.id);
            }
        }
    }

    #[test]
    fn each_scenario_request_is_versioned_and_holds_only_its_own_conversation() {
        let suite = suite();
        for case in &suite.cases {
            let (request, _) = request(&suite, case);
            assert!(request.system.starts_with(PROMPT_VERSION));
            let input = &request.messages[0].content;
            for other in &suite.cases {
                for turn in other.turns.iter().filter(|turn| turn.role == "student") {
                    let own = case.turns.iter().any(|mine| mine.content == turn.content);
                    let text = serde_json::to_string(&turn.content).unwrap();
                    assert_eq!(input.contains(&text), own, "{} / {}", case.id, other.id);
                }
            }
        }
    }

    #[test]
    fn stability_and_range_checks_follow_the_spec() {
        let run = |d1: u8, d2: u8| Scores::from([("d1".to_owned(), d1), ("d2".to_owned(), d2)]);
        let runs = [run(3, 0), run(4, 0), run(3, 2)];
        let gaps = spread(&runs);
        assert_eq!(gaps["d1"], 1);
        assert_eq!(gaps["d2"], 2);
        assert!(gaps["d1"] <= MAX_SPREAD && gaps["d2"] > MAX_SPREAD);
        let expected = BTreeMap::from([("d1".to_owned(), [3, 4]), ("d2".to_owned(), [0, 1])]);
        assert_eq!(outside_expected(&expected, &runs), ["d2"]);
    }
}
