//! 主張分組（F-15，暫定；對應 S-04）。
//!
//! 每場對話的總結有一句「最終主張」。同一題目下，AI 把意思相近的主張歸成一群並命名，
//! 分組結果存在資料庫（不在每次看儀表板時重算）：
//! - 新總結進來時，把既有群組名稱一併交給 AI，要求優先歸入既有群組，歸不進去才開新群；
//! - 教師可按「重新分組」，由 AI 把該題目的全部主張重新分一次。
//!
//! 送給 AI 的只有主張短句與題目標題，不含姓名、電子郵件或對話原文。分組失敗不影響總結本身，
//! 該場只是暫時沒有群組。

use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    AppState,
    ai::{
        AiRequest, ChatRole, ChatTurn, build_group_assign_prompt, build_regroup_prompt, collect,
        parse_group_name, parse_regroup,
    },
    auth::RequireTeacher,
    error::{ApiError, db_error_class},
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/dashboard/class/regroup", post(regroup))
}

/// 把一場對話的主張歸入群組（背景呼叫，盡力而為）。
pub async fn assign_group(state: &AppState, conversation: Uuid, topic: &str, claim: &str) {
    if let Err(e) = try_assign(state, conversation, topic, claim).await {
        tracing::warn!(class = %db_error_class(&e), "claim grouping bookkeeping failed");
    }
}

async fn try_assign(
    state: &AppState,
    conversation: Uuid,
    topic: &str,
    claim: &str,
) -> Result<(), sqlx::Error> {
    let existing: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM claim_groups WHERE topic_title = $1 ORDER BY created_at, name",
    )
    .bind(topic)
    .fetch_all(&state.pool)
    .await?;
    let existing: Vec<String> = existing.into_iter().map(|(n,)| n).collect();
    let req = AiRequest {
        system: build_group_assign_prompt(topic, &existing),
        messages: vec![ChatTurn {
            role: ChatRole::User,
            content: claim.to_string(),
        }],
    };
    let raw = collect(
        state.ai.as_ref(),
        req,
        state.config.ai_first_token_timeout,
        state.config.ai_total_timeout,
    )
    .await;
    let Some(name) = raw.ok().and_then(|r| parse_group_name(&r, &existing)) else {
        return Ok(()); // 分組失敗：這場暫時沒有群組
    };
    let group = upsert_group(&state.pool, topic, &name).await?;
    sqlx::query("UPDATE summaries SET claim_group_id = $2 WHERE conversation_id = $1")
        .bind(conversation)
        .bind(group)
        .execute(&state.pool)
        .await?;
    Ok(())
}

async fn upsert_group(pool: &PgPool, topic: &str, name: &str) -> Result<Uuid, sqlx::Error> {
    // 同時進來的兩場對話可能搶著建立同名群組：靠唯一約束，輸的人改讀現有的
    sqlx::query(
        "INSERT INTO claim_groups (topic_title, name) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(topic)
    .bind(name)
    .execute(pool)
    .await?;
    let (id,): (Uuid,) =
        sqlx::query_as("SELECT id FROM claim_groups WHERE topic_title = $1 AND name = $2")
            .bind(topic)
            .bind(name)
            .fetch_one(pool)
            .await?;
    Ok(id)
}

#[derive(Deserialize)]
struct RegroupBody {
    /// 要重新分組的題目標題（儀表板上顯示的那個）
    title: String,
}

/// 教師按「重新分組」：由 AI 把該題目下全部主張重新分一次。管理者看不到分析結果（S-02.2），不能操作。
async fn regroup(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Json(body): Json<RegroupBody>,
) -> Result<StatusCode, ApiError> {
    if cu.roles.admin {
        return Err(ApiError::forbidden("forbidden", "沒有權限"));
    }
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT s.conversation_id, s.claim
         FROM summaries s JOIN conversations c ON c.id = s.conversation_id
         WHERE s.status = 'ready' AND c.title = $1 AND s.claim IS NOT NULL
         ORDER BY c.ended_at, s.conversation_id",
    )
    .bind(&body.title)
    .fetch_all(&state.pool)
    .await?;
    if rows.is_empty() {
        return Err(ApiError::conflict(
            "no_claims",
            "這個題目還沒有可分組的主張",
        ));
    }
    let claims: Vec<String> = rows.iter().map(|(_, c)| c.clone()).collect();
    let req = AiRequest {
        system: build_regroup_prompt(&body.title, &claims),
        messages: vec![ChatTurn {
            role: ChatRole::User,
            content: "請分組。".into(),
        }],
    };
    let raw = collect(
        state.ai.as_ref(),
        req,
        state.config.ai_first_token_timeout,
        state.config.ai_total_timeout,
    )
    .await;
    let groups = raw
        .ok()
        .and_then(|r| parse_regroup(&r, claims.len()))
        .ok_or(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "ai_unavailable",
            "AI 暫時無法回覆",
        ))?;
    let mut tx = state.pool.begin().await?;
    // 舊群組刪除後，成員的 claim_group_id 會回到空，再依新結果指定
    sqlx::query("DELETE FROM claim_groups WHERE topic_title = $1")
        .bind(&body.title)
        .execute(&mut *tx)
        .await?;
    for (name, members) in groups {
        let (gid,): (Uuid,) = sqlx::query_as(
            "INSERT INTO claim_groups (topic_title, name) VALUES ($1, $2) RETURNING id",
        )
        .bind(&body.title)
        .bind(&name)
        .fetch_one(&mut *tx)
        .await?;
        let ids: Vec<Uuid> = members.into_iter().map(|i| rows[i].0).collect();
        sqlx::query("UPDATE summaries SET claim_group_id = $1 WHERE conversation_id = ANY($2)")
            .bind(gid)
            .bind(&ids)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
