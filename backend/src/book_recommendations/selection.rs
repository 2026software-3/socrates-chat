use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time::timeout;
use uuid::Uuid;

use crate::{
    AppState,
    ai::{AiError, ChatRole, ChatTurn, StructuredRequest},
    book_sources::{RecommendationEdition, RecommendationSource, isbn13},
    book_store::BookWork,
    diagnostics::AiActivity,
};

use super::store::Source;
use super::{Edition, Recommendation, RecommendationCategory};

pub(super) const MAX_WORKS: usize = 40;
pub(super) const MAX_RECOMMENDATIONS: usize = 5;
const MAX_INPUT_BYTES: usize = 48_000;

pub(super) const AI_FAILED: &str = "recommendation_ai_failed";
pub(super) const INVALID_OUTPUT: &str = "recommendation_invalid_output";
pub(super) const SOURCE_FAILED: &str = "book_source_unavailable";

impl Edition {
    fn matches(&self, language: &str) -> bool {
        self.language
            .as_deref()
            .is_some_and(|value| language_matches(value, language))
    }
}

pub(super) fn language_codes(language: &str) -> (&'static [&'static str], Option<&'static str>) {
    match language {
        "zh-TW" => (
            &[
                // Generic Chinese in ISO 639-1 and ISO 639-2, with no script specified.
                "zh",
                "chi",
                "zho",
                "zh-tw",
                "zh-hk",
                "zh-mo",
                "zh-hant",
                "中文",
                "繁體中文",
                "正體中文",
                "繁體",
            ],
            Some("zh-hant-"),
        ),
        "en" => (&["en", "eng", "英文", "英語", "english"], Some("en-")),
        "es" => (
            &[
                "es",
                "spa",
                "西文",
                "西班牙文",
                "西班牙語",
                "spanish",
                "español",
            ],
            Some("es-"),
        ),
        _ => (&[], None),
    }
}

fn language_matches(value: &str, language: &str) -> bool {
    let code = value.trim().to_lowercase().replace('_', "-");
    let (aliases, prefix) = language_codes(language);
    aliases.contains(&code.as_str()) || prefix.is_some_and(|prefix| code.starts_with(prefix))
}

pub(super) struct Candidate {
    pub(super) work_id: Option<Uuid>,
    pub(super) category: RecommendationCategory,
    pub(super) teacher_recommended: bool,
    pub(super) editions: Vec<Edition>,
}

impl Candidate {
    pub(super) fn from_work(
        work: BookWork,
        teacher_recommended: bool,
        language: &str,
    ) -> Option<Self> {
        let mut candidate = Self {
            work_id: Some(work.id),
            category: RecommendationCategory::TeacherList,
            teacher_recommended,
            editions: if work.is_active {
                work.editions
                    .into_iter()
                    .filter(|edition| edition.is_active)
                    .map(Edition::from)
                    .collect()
            } else {
                Vec::new()
            },
        };
        candidate
            .editions
            .retain(|edition| edition.matches(language));
        (!candidate.editions.is_empty()).then_some(candidate)
    }
}

fn same_edition(first: &Edition, second: &Edition) -> bool {
    first
        .isbn
        .as_deref()
        .and_then(isbn13)
        .is_some_and(|isbn| second.isbn.as_deref().and_then(isbn13).as_ref() == Some(&isbn))
        || first
            .external_sources
            .iter()
            .any(|source| second.external_sources.contains(source))
}

fn external_candidates(
    books: Vec<RecommendationEdition>,
    category: RecommendationCategory,
    local: &[Candidate],
    language: &str,
) -> Vec<Candidate> {
    let mut groups: Vec<(Option<String>, Candidate)> = Vec::new();
    for RecommendationEdition { work_id, book } in books {
        let Some(edition) = Edition::from_external(book, category) else {
            continue;
        };
        if !edition.matches(language)
            || local.iter().any(|candidate| {
                candidate
                    .editions
                    .iter()
                    .any(|other| same_edition(&edition, other))
            })
            || groups.iter().any(|(_, candidate)| {
                candidate
                    .editions
                    .iter()
                    .any(|other| same_edition(&edition, other))
            })
        {
            continue;
        }
        if let Some((_, group)) = groups
            .iter_mut()
            .find(|(key, _)| key.is_some() && *key == work_id)
        {
            group.editions.push(edition);
        } else {
            groups.push((
                work_id,
                Candidate {
                    work_id: None,
                    category,
                    teacher_recommended: false,
                    editions: vec![edition],
                },
            ));
        }
    }
    groups.into_iter().map(|(_, group)| group).collect()
}

const PROMPT: &str = include_str!("selection_prompt.txt");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pick {
    id: String,
    reason: String,
    question: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Choices {
    picks: Vec<Pick>,
    proposals: Vec<Proposal>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Proposal {
    title: String,
    author: String,
}

impl Proposal {
    fn query(&self) -> Result<String, &'static str> {
        for (value, limit) in [(&self.title, 240), (&self.author, 120)] {
            if value.trim().is_empty()
                || value.chars().count() > limit
                || value
                    .chars()
                    .any(|c| c.is_control() || matches!(c, '[' | ']' | '@' | '/' | '\\'))
                || value.contains("://")
                || crate::pii::mask(value.clone()) != *value
            {
                return Err(INVALID_OUTPUT);
            }
        }
        Ok(format!(
            "\"{}\" \"{}\"",
            self.title.replace('"', " ").trim(),
            self.author.replace('"', " ").trim()
        ))
    }

    fn matches(&self, book: &crate::book_sources::BookCandidate) -> bool {
        fn normalized(text: &str) -> String {
            text.chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect()
        }
        let title = normalized(&self.title);
        let author = normalized(&self.author);
        !title.is_empty()
            && !author.is_empty()
            && normalized(&book.title) == title
            && book.authors.as_deref().is_some_and(|names| {
                normalized(names) == author
                    || names
                        .split([',', ';', '、'])
                        .any(|name| normalized(name) == author)
            })
    }
}

#[derive(Serialize)]
struct Context<'a> {
    topic: &'a str,
    guiding_questions: &'a [String],
    language: &'a str,
    summary: Value,
    dialogue: Option<Value>,
}

fn excerpt(value: &str, limit: usize) -> &str {
    &value[..value.floor_char_boundary(limit)]
}

fn candidate_input(candidates: &[Candidate]) -> Vec<Value> {
    candidates
        .iter()
        .enumerate()
        .map(|(i, candidate)| {
            // All matching editions remain in the snapshot. Limit the material sent to the model
            // so a large catalog doesn't push out the discussion or saved summary.
            let edition = &candidate.editions[0];
            json!({
                "id": i.to_string(),
                "teacher_recommended": candidate.teacher_recommended,
                "title": excerpt(&edition.title, 240),
                "authors": edition.authors.as_deref().map(|s| excerpt(s, 100)),
                "description": edition.description.as_deref().map(|s| excerpt(s, 350)),
                "teacher_note": edition.teacher_note.as_deref().map(|s| excerpt(s, 120)),
            })
        })
        .collect()
}

fn make_request(
    source: &Source,
    turns: &[ChatTurn],
    candidates: &[Candidate],
    proposal: Option<&Proposal>,
) -> Result<StructuredRequest, &'static str> {
    let dialogue = turns.iter().map(|turn| json!({
        "role": match turn.role { ChatRole::User => "student", ChatRole::Assistant => "assistant" },
        "content": turn.content
    })).collect::<Vec<_>>();
    let mut context = Context {
        topic: &source.title,
        guiding_questions: &source.guiding_questions,
        language: &source.language,
        summary: json!({"stance":source.stance, "reasons":source.reasons,"claim":source.claim}),
        dialogue: Some(json!(dialogue)),
    };
    let budget = |value: &Value| value.to_string().len() <= MAX_INPUT_BYTES;
    let mut input = json!({
        "context": context,
        "candidates": candidate_input(candidates),
        "proposal": proposal,
        "phase": if proposal.is_some() { "external" } else { "catalog" }
    });
    if !budget(&input) {
        context.dialogue = None;
        input["context"] = json!(context);
    }
    if !budget(&input) {
        return Err(INVALID_OUTPUT);
    }
    Ok(StructuredRequest {
        name: "book_recommendations".into(),
        system: PROMPT.into(),
        input,
        schema: json!({
            "type": "object", "additionalProperties": false,
            "properties": {
                "picks": { "type": "array", "items": {
                    "type": "object", "additionalProperties": false,
                    "properties": { "id": {"type":"string"}, "reason": {"type":"string"}, "question": {"type":"string"} },
                    "required":["id","reason","question"]
                }},
                "proposals": { "type": "array", "items": {
                    "type": "object", "additionalProperties": false,
                    "properties": {"title": {"type":"string"}, "author": {"type":"string"}},
                    "required": ["title", "author"]
                }}
            },
            "required":["picks","proposals"]
        }),
    })
}

fn materialize(
    choices: Choices,
    candidates: &[Candidate],
) -> Result<(Vec<Recommendation>, Vec<Proposal>), &'static str> {
    let has_picks = !choices.picks.is_empty();
    let mut selected = HashSet::new();
    let mut results = Vec::new();
    for pick in choices.picks {
        let Some(index) = pick
            .id
            .parse::<usize>()
            .ok()
            .filter(|i| i.to_string() == pick.id && *i < candidates.len())
        else {
            continue;
        };
        let (reason, question) = (pick.reason.trim(), pick.question.trim());
        if reason.is_empty()
            || question.is_empty()
            || reason.chars().count() > 1000
            || question.chars().count() > 500
            || !selected.insert(index)
        {
            continue;
        }
        let candidate = &candidates[index];
        results.push(Recommendation {
            work_id: candidate.work_id,
            source: candidate.category,
            teacher_recommended: candidate.teacher_recommended,
            editions: candidate.editions.clone(),
            reason: reason.to_string(),
            question: question.to_string(),
        });
    }
    if has_picks && results.is_empty() {
        return Err(INVALID_OUTPUT);
    }
    // Preserve the model's relevance order within a source priority.
    results.sort_by_key(|book| !book.teacher_recommended);
    results.truncate(MAX_RECOMMENDATIONS);
    if choices.proposals.len() > MAX_RECOMMENDATIONS {
        return Err(INVALID_OUTPUT);
    }
    for proposal in &choices.proposals {
        proposal.query()?;
    }
    Ok((results, choices.proposals))
}

async fn choose(
    state: &AppState,
    source: &Source,
    turns: &[ChatTurn],
    candidates: &[Candidate],
    proposal: Option<&Proposal>,
) -> Result<(Vec<Recommendation>, Vec<Proposal>), &'static str> {
    let req = make_request(source, turns, candidates, proposal)?;
    let activity = AiActivity::new(
        "book_recommendations",
        Some(source.user_id),
        source.classroom_id,
    )
    .statuses(202, 202);
    let route = state
        .ai_for_conversation(source.classroom_id, Some(source.id))
        .await
        .map_err(|error| {
            activity.record_unreached(state, &error);
            AI_FAILED
        })?;
    let choices = activity
        .run(state, Some(&route.model), 0, async {
            let completion = timeout(state.config.ai_total_timeout, route.ai.complete_json(req))
                .await
                .map_err(|_| AiError::classified(crate::diagnostics::Failure::Timeout))??;
            serde_json::from_str(&completion.content).map_err(|_| AiError::invalid_output())
        })
        .await
        .map_err(|error| {
            if error.class == crate::diagnostics::Failure::InvalidOutput {
                INVALID_OUTPUT
            } else {
                AI_FAILED
            }
        })?;
    materialize(choices, candidates)
}

pub(super) async fn generate(
    state: &AppState,
    source: &Source,
    turns: &[ChatTurn],
    local: &[Candidate],
) -> Result<Vec<Recommendation>, &'static str> {
    let (mut results, proposals) = choose(state, source, turns, local, None).await?;
    if results
        .iter()
        .filter(|book| book.teacher_recommended)
        .count()
        == MAX_RECOMMENDATIONS
    {
        return Ok(results);
    }
    let mut searched = HashSet::new();
    let mut verified = Vec::new();
    let mut external_results = Vec::new();
    for proposal in proposals {
        let query = proposal.query()?;
        if !searched.insert(query.clone()) {
            continue;
        }
        for (provider, category) in [
            (
                RecommendationSource::GoogleBooks,
                RecommendationCategory::GoogleBooks,
            ),
            (
                RecommendationSource::OpenLibrary,
                RecommendationCategory::OpenLibrary,
            ),
        ] {
            let books = timeout(
                std::time::Duration::from_secs(10),
                state.book_source.search_recommendations(provider, &query),
            )
            .await
            .map_err(|_| SOURCE_FAILED)?
            .map_err(|_| SOURCE_FAILED)?;
            let books = books
                .into_iter()
                .filter(|edition| proposal.matches(&edition.book))
                .collect();
            let candidates = external_candidates(books, category, local, &source.language);
            let candidates: Vec<_> = candidates
                .into_iter()
                .filter(|candidate| {
                    !verified.iter().any(|other: &Edition| {
                        candidate
                            .editions
                            .iter()
                            .any(|edition| same_edition(edition, other))
                    })
                })
                .collect();
            if candidates.is_empty() {
                continue;
            }
            let (additional, unexpected) =
                choose(state, source, turns, &candidates, Some(&proposal)).await?;
            if !unexpected.is_empty() {
                return Err(INVALID_OUTPUT);
            }
            if !additional.is_empty() {
                verified.extend(
                    additional
                        .iter()
                        .flat_map(|book| book.editions.iter().cloned()),
                );
                external_results.extend(additional);
                break;
            }
        }
    }
    let first_shared = results.partition_point(|book| book.teacher_recommended);
    results.splice(first_shared..first_shared, external_results);
    results.truncate(MAX_RECOMMENDATIONS);
    Ok(results)
}

#[cfg(test)]
#[path = "evaluation.rs"]
mod evaluation;
