//! Student-owned, on-demand summary trees. The persisted format is distinct from S-04.4.
use std::time::Instant;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{
        StatusCode,
        header::{CACHE_CONTROL, HeaderValue},
    },
    middleware,
    response::Response,
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    AppState,
    ai::StructuredRequest,
    auth::RequireStudent,
    chat::{load_messages, owned_conversation},
    error::{ApiError, db_error_class},
    main_argument_tree::{
        OUTPUT_SCHEMA, PROMPT_VERSION, SYSTEM_PROMPT, Transcript, model_input, parse_and_build,
        parse_transcript, sanitized_transcript,
    },
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/conversations/{id}/argument-tree",
            get(read).post(create),
        )
        .layer(middleware::map_response(
            |mut response: Response| async move {
                response
                    .headers_mut()
                    .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
                response
            },
        ))
}

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub struct ArgumentTreeView {
    /// missing, pending, ready, or failed.
    pub status: String,
    /// Validated DisplayTree JSON; null unless ready. Never includes the raw provider response.
    pub tree: Option<Value>,
    pub prompt_version: Option<String>,
    pub completed_at: Option<DateTime<Utc>>,
}

async fn view(state: &AppState, id: Uuid) -> Result<ArgumentTreeView, ApiError> {
    // A process may have restarted while a job was pending. Expire only after its full deadline + grace.
    let stale_secs = state.config.ai_total_timeout.as_secs_f64() + 30.0;
    sqlx::query("UPDATE conversation_argument_trees SET status = 'failed', completed_at = now(), updated_at = now()
                 WHERE conversation_id = $1 AND status = 'pending'
                   AND updated_at < now() - make_interval(secs => $2)")
        .bind(id).bind(stale_secs).execute(&state.pool).await?;
    Ok(sqlx::query_as::<_, ArgumentTreeView>(
        "SELECT status, tree, prompt_version, completed_at FROM conversation_argument_trees WHERE conversation_id = $1")
        .bind(id).fetch_optional(&state.pool).await?
        .unwrap_or(ArgumentTreeView { status: "missing".into(), tree: None, prompt_version: None, completed_at: None }))
}

async fn check_owner(
    state: &AppState,
    cu: &crate::auth::CurrentUser,
    id: Uuid,
) -> Result<crate::chat::Conversation, ApiError> {
    if cu.roles.admin {
        return Err(ApiError::forbidden("forbidden", "沒有權限"));
    }
    let conversation = owned_conversation(state, cu.user.id, id).await?;
    if conversation.status != "ended" {
        return Err(ApiError::conflict("conversation_not_ended", "請先結束對話"));
    }
    Ok(conversation)
}

#[utoipa::path(
    get, path = "/api/conversations/{id}/argument-tree", tag = "conversations",
    summary = "學生讀取自己已結束對話的摘要論證樹",
    params(("id" = Uuid, Path, description = "對話 ID")),
    responses(
        (status = 200, description = "missing/pending/ready/failed；只有 ready 包含 tree", body = ArgumentTreeView),
        (status = 409, description = "conversation_not_ended", body = crate::error::ErrorBody),
        crate::error::CommonErrors
    )
)]
pub(crate) async fn read(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ArgumentTreeView>, ApiError> {
    check_owner(&state, &cu, id).await?;
    Ok(Json(view(&state, id).await?))
}

#[utoipa::path(
    post, path = "/api/conversations/{id}/argument-tree", tag = "conversations",
    summary = "按需產生並保存摘要論證樹；已完成或產生中時重用結果",
    description = "只讀取伺服器保存的對話；不接受前端逐字稿。回 202 後輪詢 GET 同一路徑。失敗可再 POST；已完成結果不重算。計入 AI 額度，失敗退回。",
    params(("id" = Uuid, Path, description = "對話 ID")),
    responses(
        (status = 200, description = "重用已保存的結果", body = ArgumentTreeView),
        (status = 202, description = "背景產生中", body = ArgumentTreeView),
        (status = 409, description = "conversation_not_ended / classroom_archived / empty_conversation / argument_tree_input_invalid", body = crate::error::ErrorBody),
        (status = 429, description = "ai_usage_exceeded", body = crate::error::ErrorBody),
        crate::error::CommonErrors
    )
)]
pub(crate) async fn create(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<ArgumentTreeView>), ApiError> {
    let conversation = check_owner(&state, &cu, id).await?;
    let current = view(&state, id).await?;
    if current.status == "ready" || current.status == "pending" {
        return Ok(response(current));
    }
    let messages = load_messages(&state, id).await?;
    if !messages.iter().any(|m| m.role == "student") {
        return Err(ApiError::conflict("empty_conversation", "沒有學生發言"));
    }
    // Node IDs are scoped to the result; conversation identity stays in the authenticated URL.
    let input = parse_transcript(&json!({
        "topic_id":"TREE", "title":conversation.title, "description":conversation.description, "messages":messages
    }).to_string()).map_err(|_| ApiError::conflict("argument_tree_input_invalid", "對話格式無法分析"))?;
    let input = sanitized_transcript(&input).map_err(|_| ApiError::internal())?;
    let mut tx = state.pool.begin().await?;
    // Keep the archive check and the generation claim in one short transaction.
    crate::classrooms::writable_conversation(&mut tx, cu.user.id, id).await?;
    let attempt: Option<i32> = sqlx::query_scalar(
        "INSERT INTO conversation_argument_trees (conversation_id, status, attempt, prompt_version)
         VALUES ($1, 'pending', 1, $2)
         ON CONFLICT (conversation_id) DO UPDATE
           SET status = 'pending', attempt = conversation_argument_trees.attempt + 1,
               prompt_version = EXCLUDED.prompt_version, tree = NULL, model = NULL,
               updated_at = now(), completed_at = NULL
           WHERE conversation_argument_trees.status = 'failed'
         RETURNING attempt",
    )
    .bind(id)
    .bind(PROMPT_VERSION)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(attempt) = attempt else {
        tx.commit().await?;
        return Ok(response(view(&state, id).await?));
    };
    // Only the winning request pays. A quota error rolls back the pending claim.
    let slot = state.charge_ai(cu.user.id)?;
    let mut charge = AiCharge {
        state: state.clone(),
        user: cu.user.id,
        slot: Some(slot),
    };
    tx.commit().await?;
    let language = conversation.language;
    tokio::spawn(async move {
        if let Err(error) = generate(&state, id, attempt, input, language, &mut charge).await {
            tracing::error!(class = %db_error_class(&error), "argument tree generation bookkeeping failed");
            let _ = sqlx::query("UPDATE conversation_argument_trees SET status = 'failed', updated_at = now(), completed_at = now()
                                WHERE conversation_id = $1 AND attempt = $2 AND status = 'pending'")
                .bind(id).bind(attempt).execute(&state.pool).await;
        }
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(ArgumentTreeView {
            status: "pending".into(),
            tree: None,
            prompt_version: Some(PROMPT_VERSION.into()),
            completed_at: None,
        }),
    ))
}

fn response(value: ArgumentTreeView) -> (StatusCode, Json<ArgumentTreeView>) {
    (
        if value.status == "pending" {
            StatusCode::ACCEPTED
        } else {
            StatusCode::OK
        },
        Json(value),
    )
}

/// Refund on timeout, provider/validation/database failure, cancellation or a superseded attempt.
struct AiCharge {
    state: AppState,
    user: Uuid,
    slot: Option<Instant>,
}
impl Drop for AiCharge {
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            self.state.refund_ai(self.user, slot);
        }
    }
}

async fn generate(
    state: &AppState,
    id: Uuid,
    attempt: i32,
    input: Transcript,
    language: String,
    charge: &mut AiCharge,
) -> Result<(), sqlx::Error> {
    let request = model_input(&input).ok().and_then(|input| serde_json::from_str(OUTPUT_SCHEMA).ok().map(|schema| StructuredRequest {
        system: format!("{SYSTEM_PROMPT}\n請以對話語言 {language} 撰寫 title 與 label；evidence 仍須逐字引用，不翻譯。"),
        input, schema, name: "main_argument_tree".into(),
    }));
    let completion = if let Some(request) = request {
        tokio::time::timeout(
            state.config.ai_total_timeout,
            state.ai.complete_json(request),
        )
        .await
        .ok()
        .and_then(Result::ok)
    } else {
        None
    };
    if let Some(completion) = completion
        && let Ok((_, tree)) = parse_and_build(&input, &completion.content)
        && let Ok(tree) = serde_json::to_value(tree)
    {
        let updated = sqlx::query(
            "UPDATE conversation_argument_trees SET status = 'ready', tree = $3, model = $4, updated_at = now(), completed_at = now()
             WHERE conversation_id = $1 AND attempt = $2 AND status = 'pending'")
            .bind(id).bind(attempt).bind(tree).bind(completion.model).execute(&state.pool).await?;
        if updated.rows_affected() > 0 {
            charge.slot = None;
        }
    } else {
        sqlx::query("UPDATE conversation_argument_trees SET status = 'failed', updated_at = now(), completed_at = now()
                     WHERE conversation_id = $1 AND attempt = $2 AND status = 'pending'")
            .bind(id).bind(attempt).execute(&state.pool).await?;
    }
    Ok(())
}
