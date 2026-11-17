//! Translation providers receive only the two permitted summary fields (S-12.1).

use std::time::Duration;

use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    AppState,
    ai::{
        AiError, AiRequest, ChatRole, ChatTurn, TranslatedSummary,
        build_summary_translation_prompt, collect_background, parse_summary_translation,
        validated_summary_translation,
    },
    google_translation::TIMEOUT as GOOGLE_TIMEOUT,
    summary::MASKED,
};

const BATCH_SIZE: usize = 16;

pub fn spawn_worker(state: AppState, retry_delay: Duration) {
    tokio::spawn(async move {
        loop {
            match process_batch(&state, retry_delay).await {
                Ok(BATCH_SIZE) => continue,
                Ok(_) => {}
                Err(error) => {
                    tracing::error!(class = %crate::error::db_error_class(&error), "summary translation job failed")
                }
            }
            tokio::select! {
                () = tokio::time::sleep(retry_delay) => {},
                () = state.summary_translation_wake.notified() => {},
            }
        }
    });
}

/// Discover new and pre-existing summaries without touching their original data.
pub async fn process_batch(state: &AppState, retry_delay: Duration) -> Result<usize, sqlx::Error> {
    sqlx::query(
        "INSERT INTO summary_translations (conversation_id, language)
         SELECT s.conversation_id, target.language
         FROM summaries s
         JOIN conversations c ON c.id = s.conversation_id
         CROSS JOIN (VALUES ('zh-TW'), ('en'), ('es')) AS target(language)
         WHERE s.status = 'ready' AND target.language <> c.language
           AND NOT EXISTS (
               SELECT 1 FROM summary_translations t
               WHERE t.conversation_id = s.conversation_id AND t.language = target.language
           )
         ON CONFLICT (conversation_id, language) DO NOTHING",
    )
    .execute(&state.pool)
    .await?;

    for count in 0..BATCH_SIZE {
        if !process_next(state, retry_delay).await? {
            return Ok(count);
        }
    }
    Ok(BATCH_SIZE)
}

#[derive(FromRow)]
struct Job {
    conversation_id: Uuid,
    source_language: String,
    language: String,
    stance: String,
    reasons: String,
}

async fn process_next(state: &AppState, retry_delay: Duration) -> Result<bool, sqlx::Error> {
    // Claim the version for longer than the bounded provider calls. Deletion can proceed
    // during translation, and a crashed worker's claim expires without startup cleanup.
    let token = Uuid::new_v4();
    let lease = GOOGLE_TIMEOUT + state.config.ai_total_timeout + Duration::from_secs(5);
    let job: Option<Job> = sqlx::query_as(
        "WITH next_job AS (
             SELECT t.conversation_id, c.language AS source_language, t.language, s.stance, s.reasons
             FROM summary_translations t
             JOIN summaries s ON s.conversation_id = t.conversation_id
             JOIN conversations c ON c.id = t.conversation_id
             WHERE t.stance IS NULL AND t.available_at <= now() AND s.status = 'ready'
             ORDER BY t.available_at, t.conversation_id, t.language
             LIMIT 1 FOR UPDATE OF t SKIP LOCKED
         )
         UPDATE summary_translations t
         SET attempt_token = $1, available_at = clock_timestamp() + make_interval(secs => $2)
         FROM next_job j
         WHERE t.conversation_id = j.conversation_id AND t.language = j.language
         RETURNING j.conversation_id, j.source_language, j.language, j.stance, j.reasons",
    )
    .bind(token)
    .bind(lease.as_secs_f64())
    .fetch_optional(&state.pool)
    .await?;
    let Some(job) = job else {
        return Ok(false);
    };
    let [stance, reasons] = [job.stance, job.reasons].map(crate::pii::mask);
    let fields = [&*stance, &*reasons];
    let result = match via_google(state, &job.source_language, &job.language, fields).await {
        Some(translation) => Some(translation),
        None => via_ai(state, &job.language, fields).await.ok(),
    };
    let (stance, reasons) = result.map_or((None, None), |result| {
        (Some(result.stance), Some(result.reasons))
    });
    sqlx::query(
        "UPDATE summary_translations SET stance = $3, reasons = $4, attempt_token = NULL,
                available_at = clock_timestamp() + make_interval(secs => $5)
         WHERE conversation_id = $1 AND language = $2 AND attempt_token = $6 AND stance IS NULL",
    )
    .bind(job.conversation_id)
    .bind(job.language)
    .bind(stance)
    .bind(reasons)
    .bind(retry_delay.as_secs_f64())
    .bind(token)
    .execute(&state.pool)
    .await?;
    Ok(true)
}

pub(crate) async fn via_google(
    state: &AppState,
    source: &str,
    target: &str,
    fields: [&str; 2],
) -> Option<TranslatedSummary> {
    let [stance, reasons] = state
        .google_translation
        .as_ref()?
        .translate(source, target, fields)
        .await?;
    let result = validated_summary_translation(stance, reasons)?;
    preserves_tokens(fields, &result).then_some(result)
}

pub(crate) async fn via_ai(
    state: &AppState,
    target: &str,
    fields: [&str; 2],
) -> Result<TranslatedSummary, AiError> {
    let [stance, reasons] = fields;
    let req = AiRequest {
        system: format!(
            "{} Keep [EMAIL], [PHONE], [NATIONAL_ID], [STUDENT_ID] and the fixed masking field 'message' exactly as provided.",
            build_summary_translation_prompt(target),
        ),
        messages: vec![ChatTurn {
            role: ChatRole::User,
            content: serde_json::json!({"stance": stance, "reasons": reasons}).to_string(),
        }],
    };
    let raw = collect_background(state.ai.as_ref(), req, state.config.ai_total_timeout).await?;
    let result = parse_summary_translation(&raw).ok_or_else(AiError::invalid_output)?;
    preserves_tokens(fields, &result)
        .then_some(result)
        .ok_or_else(AiError::invalid_output)
}

fn preserves_tokens(fields: [&str; 2], result: &TranslatedSummary) -> bool {
    const TOKENS: [&str; 4] = ["[EMAIL]", "[PHONE]", "[NATIONAL_ID]", "[STUDENT_ID]"];
    fields
        .into_iter()
        .zip([&result.stance, &result.reasons])
        .all(|(source, translated)| {
            (source != MASKED || translated == MASKED)
                && TOKENS
                    .iter()
                    .all(|token| source.matches(token).count() == translated.matches(token).count())
        })
}
