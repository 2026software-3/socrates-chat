//! 主張分組（F-15，暫定；對應 S-04.7）。
//!
//! 每場對話的總結有一句「最終主張」。同一個活動的同一題目下，AI 把意思相近的主張歸成一群並命名，
//! 分組結果存在資料庫（不在每次看儀表板時重算）：
//! - 新總結進來時，把該活動該題目既有群組名稱一併交給 AI，要求優先歸入既有群組，歸不進去才開新群；
//! - 教師可按「重新分組」，由 AI 把該活動該題目的全部主張重新分一次。
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
        AiError, AiRequest, ChatRole, ChatTurn, build_group_assign_prompt, build_regroup_prompt,
        collect_background, parse_group_name, parse_regroup, regroup_user_message,
    },
    auth::RequireTeacher,
    diagnostics::AiActivity,
    error::{ApiError, db_error_class},
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/dashboard/class/regroup", post(regroup))
}

/// 把一場對話的主張歸入群組（背景呼叫，盡力而為）。
///
/// 群組名稱是給教師看的，而且同一活動的學生可能各用不同語言，所以名稱一律用教室擁有者
/// （建立教室的教師）的語言，不用學生自己的對話語言；重新分組用的是按下按鈕的教師的語言，
/// 兩者在同一間教室內是同一個人。
///
/// 已知限制：教師中途換了介面語言時，新主張會以新語言命名，既有群組的名稱不會跟著改，
/// 同一活動可能暫時有兩種語言的近似群組，直到教師按「重新分組」。名稱存的是文字而不是
/// 翻譯鍵，要自動改名得再呼叫一次 AI，所以這裡不做。
pub async fn assign_group(
    state: &AppState,
    conversation: Uuid,
    activity: Uuid,
    topic: &str,
    claim: &str,
) {
    if let Err(e) = try_assign(state, conversation, activity, topic, claim).await {
        tracing::warn!(class = %db_error_class(&e), "claim grouping bookkeeping failed");
    }
}

#[derive(sqlx::FromRow)]
struct AssignSource {
    language: String,
    user_id: Option<Uuid>,
    classroom_id: Uuid,
}

async fn try_assign(
    state: &AppState,
    conversation: Uuid,
    activity: Uuid,
    topic: &str,
    claim: &str,
) -> Result<(), sqlx::Error> {
    let topic_id: Option<Uuid> =
        sqlx::query_scalar("SELECT topic_id FROM conversations WHERE id = $1 AND activity_id = $2")
            .bind(conversation)
            .bind(activity)
            .fetch_optional(&state.pool)
            .await?
            .flatten();
    let Some(topic_id) = topic_id else {
        return Ok(());
    };
    let (source, existing): (Option<AssignSource>, Vec<(String,)>) = tokio::try_join!(
        sqlx::query_as(
            "SELECT u.language, c.user_id, a.classroom_id
             FROM activities a
             JOIN classrooms cl ON cl.id = a.classroom_id
             JOIN users u ON u.id = cl.created_by
             LEFT JOIN conversations c ON c.activity_id = a.id AND c.id = $2
             WHERE a.id = $1",
        )
        .bind(activity)
        .bind(conversation)
        .fetch_optional(&state.pool),
        sqlx::query_as(
            "SELECT name FROM claim_groups WHERE activity_id = $1 AND topic_id = $2 ORDER BY created_at, name",
        )
        .bind(activity)
        .bind(topic_id)
        .fetch_all(&state.pool),
    )?;
    let (language, user, classroom) = match source {
        Some(source) => (source.language, source.user_id, Some(source.classroom_id)),
        None => ("zh-TW".to_owned(), None, None),
    };
    let existing: Vec<String> = existing.into_iter().map(|(n,)| n).collect();
    let req = AiRequest {
        system: build_group_assign_prompt(topic, &existing, &language),
        messages: vec![ChatTurn {
            role: ChatRole::User,
            content: claim.to_string(),
        }],
    };
    let grouped = AiActivity::new("claim_group", user, classroom)
        .statuses(202, 202)
        .run(state, Some(&state.config.openai_model), 0, async {
            let raw =
                collect_background(state.ai.as_ref(), req, state.config.ai_total_timeout).await?;
            parse_group_name(&raw, &existing).ok_or(AiError::invalid_output())
        })
        .await;
    let Ok(name) = grouped else {
        return Ok(()); // 分組失敗：這場暫時沒有群組
    };
    let group = upsert_group(&state.pool, activity, topic_id, &name).await?;
    sqlx::query("UPDATE summaries SET claim_group_id = $2 WHERE conversation_id = $1")
        .bind(conversation)
        .bind(group)
        .execute(&state.pool)
        .await?;
    Ok(())
}

async fn upsert_group(
    pool: &PgPool,
    activity: Uuid,
    topic_id: Uuid,
    name: &str,
) -> Result<Uuid, sqlx::Error> {
    // 同時進來的兩場對話可能搶著建立同名群組：靠唯一約束，輸的人改讀現有的
    sqlx::query(
        "INSERT INTO claim_groups (activity_id, topic_id, name) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(activity)
    .bind(topic_id)
    .bind(name)
    .execute(pool)
    .await?;
    let (id,): (Uuid,) = sqlx::query_as(
        "SELECT id FROM claim_groups WHERE activity_id = $1 AND topic_id = $2 AND name = $3",
    )
    .bind(activity)
    .bind(topic_id)
    .bind(name)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct RegroupBody {
    /// 要重新分組的活動（儀表板上顯示的那個）
    activity_id: Uuid,
    /// 活動內要重新分組的題目
    topic_id: Uuid,
}

/// 教師按「重新分組」：由 AI 把該活動指定題目的全部主張重新分一次。管理者看不到分析結果（S-02.2），不能操作。
#[utoipa::path(
    post,
    path = "/api/dashboard/class/regroup",
    tag = "dashboard",
    summary = "請 AI 重新分組某活動題目的主張",
    request_body = RegroupBody,
    responses((status = 204, description = "已重新分組"), (status = 404, description = "not_found：活動不存在、不在自己管理的教室，或題目不屬於活動", body = crate::error::ErrorBody), (status = 409, description = "classroom_archived / no_claims", body = crate::error::ErrorBody), (status = 502, description = "ai_unavailable", body = crate::error::ErrorBody))
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
    if !activity.topic_ids.contains(&body.topic_id) {
        return Err(ApiError::not_found());
    }
    crate::classrooms::ensure_writable(&state.pool, activity.classroom_id).await?;
    let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT s.conversation_id, s.claim, t.title
         FROM summaries s
         JOIN conversations c ON c.id = s.conversation_id
         JOIN topics t ON t.id = c.topic_id
         WHERE s.status = 'ready' AND c.activity_id = $1 AND c.topic_id = $2 AND s.claim IS NOT NULL
         ORDER BY c.ended_at, s.conversation_id",
    )
    .bind(body.activity_id)
    .bind(body.topic_id)
    .fetch_all(&state.pool)
    .await?;
    let Some((_, _, title)) = rows.first() else {
        return Err(ApiError::conflict(
            "no_claims",
            "這道題目還沒有可分組的主張",
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
    // 教師按下「重新分組」才發生的 AI 呼叫，計入每位使用者的 AI 用量；沒有得到分組就退回
    let usage_slot = state.charge_ai(cu.user.id)?;
    let groups = AiActivity::new(
        "claim_regroup",
        Some(cu.user.id),
        Some(activity.classroom_id),
    )
    .statuses(204, 502)
    .run(&state, Some(&state.config.openai_model), 0, async {
        let raw = collect_background(state.ai.as_ref(), req, state.config.ai_total_timeout).await?;
        parse_regroup(&raw, claims.len()).ok_or(AiError::invalid_output())
    })
    .await
    .map_err(|_| {
        state.refund_ai(cu.user.id, usage_slot);
        ApiError::new(StatusCode::BAD_GATEWAY, "ai_unavailable", "AI 暫時無法回覆")
    })?;
    let mut tx = state.pool.begin().await?;
    // AI 呼叫期間教室可能被封存：寫入前在交易內再確認一次
    crate::classrooms::ensure_writable(&mut *tx, activity.classroom_id).await?;
    // 舊群組刪除後，成員的 claim_group_id 會回到空，再依新結果指定
    sqlx::query("DELETE FROM claim_groups WHERE activity_id = $1 AND topic_id = $2")
        .bind(body.activity_id)
        .bind(body.topic_id)
        .execute(&mut *tx)
        .await?;
    for (name, members) in groups {
        let (gid,): (Uuid,) = sqlx::query_as(
            "INSERT INTO claim_groups (activity_id, topic_id, name) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(body.activity_id)
        .bind(body.topic_id)
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
