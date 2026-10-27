//! 主張分組（F-15，暫定；對應 S-04.7）。
//!
//! 每場對話的總結有一句「最終主張」。同一個活動下，AI 把意思相近的主張歸成一群並命名，
//! 分組結果存在資料庫（不在每次看儀表板時重算）：
//! - 新總結進來時，把該活動既有群組名稱一併交給 AI，要求優先歸入既有群組，歸不進去才開新群；
//! - 教師可按「重新分組」，由 AI 把該活動的全部主張重新分一次。
//!
//! 從題目庫開始的個人練習沒有活動，不分組。
//!
//! 送給 AI 的只有主張短句與題目標題，不含姓名、電子郵件或對話原文。分組失敗不影響總結本身，
//! 該場只是暫時沒有群組。

use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    AppState, activity_store,
    ai::{
        AiRequest, ChatRole, ChatTurn, build_group_assign_prompt, build_regroup_prompt,
        collect_background, parse_group_name, parse_regroup, regroup_user_message,
    },
    auth::RequireTeacher,
    error::{ApiError, db_error_class},
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/dashboard/class/regroup", post(regroup))
}

/// 把一場對話的主張歸入群組（背景呼叫，盡力而為）。
pub async fn assign_group(
    state: &AppState,
    conversation: Uuid,
    activity: Uuid,
    topic: &str,
    claim: &str,
    language: &str,
) {
    if let Err(e) = try_assign(state, conversation, activity, topic, claim, language).await {
        tracing::warn!(class = %db_error_class(&e), "claim grouping bookkeeping failed");
    }
}

async fn try_assign(
    state: &AppState,
    conversation: Uuid,
    activity: Uuid,
    topic: &str,
    claim: &str,
    language: &str,
) -> Result<(), sqlx::Error> {
    let existing: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM claim_groups WHERE activity_id = $1 ORDER BY created_at, name",
    )
    .bind(activity)
    .fetch_all(&state.pool)
    .await?;
    let existing: Vec<String> = existing.into_iter().map(|(n,)| n).collect();
    let req = AiRequest {
        system: build_group_assign_prompt(topic, &existing, language),
        messages: vec![ChatTurn {
            role: ChatRole::User,
            content: claim.to_string(),
        }],
    };
    let raw = collect_background(state.ai.as_ref(), req, state.config.ai_total_timeout).await;
    let Some(name) = raw.ok().and_then(|r| parse_group_name(&r, &existing)) else {
        return Ok(()); // 分組失敗：這場暫時沒有群組
    };
    let group = upsert_group(&state.pool, activity, &name).await?;
    sqlx::query("UPDATE summaries SET claim_group_id = $2 WHERE conversation_id = $1")
        .bind(conversation)
        .bind(group)
        .execute(&state.pool)
        .await?;
    Ok(())
}

async fn upsert_group(pool: &PgPool, activity: Uuid, name: &str) -> Result<Uuid, sqlx::Error> {
    // 同時進來的兩場對話可能搶著建立同名群組：靠唯一約束，輸的人改讀現有的
    sqlx::query(
        "INSERT INTO claim_groups (activity_id, name) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(activity)
    .bind(name)
    .execute(pool)
    .await?;
    let (id,): (Uuid,) =
        sqlx::query_as("SELECT id FROM claim_groups WHERE activity_id = $1 AND name = $2")
            .bind(activity)
            .bind(name)
            .fetch_one(pool)
            .await?;
    Ok(id)
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct RegroupBody {
    /// 要重新分組的活動（儀表板上顯示的那個）
    activity_id: Uuid,
}

/// 教師按「重新分組」：由 AI 把該活動下全部主張重新分一次。管理者看不到分析結果（S-02.2），不能操作。
#[utoipa::path(
    post,
    path = "/api/dashboard/class/regroup",
    tag = "dashboard",
    summary = "請 AI 重新分組某活動的主張",
    request_body = RegroupBody,
    responses((status = 204, description = "已重新分組"), (status = 404, description = "not_found：活動不存在或不在自己管理的教室", body = crate::error::ErrorBody), (status = 409, description = "classroom_archived / no_claims", body = crate::error::ErrorBody), (status = 502, description = "ai_unavailable", body = crate::error::ErrorBody))
)]
pub(crate) async fn regroup(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Json(body): Json<RegroupBody>,
) -> Result<StatusCode, ApiError> {
    if cu.roles.admin {
        return Err(ApiError::forbidden("forbidden", "沒有權限"));
    }
    // 只能重新分組自己教室內的活動；他人的與不存在的活動回應相同
    let activity = activity_store::get_visible(&state.pool, body.activity_id, cu.user.id, false)
        .await?
        .ok_or_else(ApiError::not_found)?;
    crate::classrooms::ensure_writable(&state.pool, activity.classroom_id).await?;
    let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT s.conversation_id, s.claim, a.title
         FROM summaries s
         JOIN conversations c ON c.id = s.conversation_id
         JOIN activities a ON a.id = c.activity_id
         WHERE s.status = 'ready' AND c.activity_id = $1 AND s.claim IS NOT NULL
         ORDER BY c.ended_at, s.conversation_id",
    )
    .bind(body.activity_id)
    .fetch_all(&state.pool)
    .await?;
    let Some((_, _, title)) = rows.first() else {
        return Err(ApiError::conflict(
            "no_claims",
            "這個活動還沒有可分組的主張",
        ));
    };
    let title = title.clone();
    // 群組名稱是給教師看的，用教師的語言；學生各自的對話語言不同，不能拿來決定
    let language = cu.user.language.as_str();
    let claims: Vec<String> = rows.iter().map(|(_, c, _)| c.clone()).collect();
    let req = AiRequest {
        system: build_regroup_prompt(&title, &claims, language),
        messages: vec![ChatTurn {
            role: ChatRole::User,
            content: regroup_user_message(language).into(),
        }],
    };
    let raw = collect_background(state.ai.as_ref(), req, state.config.ai_total_timeout).await;
    let groups = raw
        .ok()
        .and_then(|r| parse_regroup(&r, claims.len()))
        .ok_or(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "ai_unavailable",
            "AI 暫時無法回覆",
        ))?;
    let mut tx = state.pool.begin().await?;
    // AI 呼叫期間教室可能被封存：寫入前在交易內再確認一次
    crate::classrooms::ensure_writable(&mut *tx, activity.classroom_id).await?;
    // 舊群組刪除後，成員的 claim_group_id 會回到空，再依新結果指定
    sqlx::query("DELETE FROM claim_groups WHERE activity_id = $1")
        .bind(body.activity_id)
        .execute(&mut *tx)
        .await?;
    for (name, members) in groups {
        let (gid,): (Uuid,) = sqlx::query_as(
            "INSERT INTO claim_groups (activity_id, name) VALUES ($1, $2) RETURNING id",
        )
        .bind(body.activity_id)
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
