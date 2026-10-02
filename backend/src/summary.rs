//! 結束對話與 AI 總結（F-08、F-26；規格見 S-03.3、S-02.1、S-02.2）。
//!
//! 對話結束後在背景產生總結；學生輪詢 `GET .../summary` 看狀態。AI 失敗或結果不完整時
//! 狀態為 `failed`，可用重試端點再試一次（F-08.4）。總結不可由學生修改。
//! 教師只看得到總結，看不到對話原文；管理者介面一律遮蔽（以 `message` 取代）。

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgConnection};
use uuid::Uuid;

use crate::{
    AppState,
    ai::{
        AiRequest, ChatRole, ChatTurn, RULES_VERSION, build_summary_prompt, collect,
        format_transcript, parse_summary,
    },
    auth::{RequireStudent, RequireTeacher},
    chat::owned_conversation,
    error::{ApiError, db_error_class},
};

/// 管理者介面看到的固定替代字串（S-02.2）。
pub const MASKED: &str = "message";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/conversations/{id}/end", post(end))
        .route("/api/conversations/{id}/summary", get(get_summary))
        .route("/api/conversations/{id}/summary/retry", post(retry))
        .route("/api/teacher/summaries", get(teacher_list))
}

/// 結束對話並建立「產生中」的總結；回傳是否真的由這次呼叫結束。
///
/// 已結束，或 AI 正在回覆中（要等回覆完成，總結才包含學生看到的全部內容）時回傳 `false`。
pub async fn end_conversation(
    conn: &mut PgConnection,
    id: Uuid,
    stale_secs: f64,
) -> Result<bool, sqlx::Error> {
    let r = sqlx::query(
        "UPDATE conversations SET status = 'ended', ended_at = now()
         WHERE id = $1 AND status = 'active'
           AND (generating_since IS NULL
                OR generating_since < now() - make_interval(secs => $2))",
    )
    .bind(id)
    .bind(stale_secs)
    .execute(&mut *conn)
    .await?;
    if r.rows_affected() == 0 {
        return Ok(false);
    }
    sqlx::query("INSERT INTO summaries (conversation_id) VALUES ($1)")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    Ok(true)
}

/// 服務啟動時呼叫：背景任務不會跨重啟存活，所以此時還是 pending 的總結都是孤兒，
/// 標記為 failed 讓學生可以重試。
pub async fn recover_orphaned(pool: &sqlx::PgPool) -> Result<u64, sqlx::Error> {
    let r = sqlx::query(
        "UPDATE summaries SET status = 'failed', completed_at = now() WHERE status = 'pending'",
    )
    .execute(pool)
    .await?;
    Ok(r.rows_affected())
}

/// 在背景產生第 `attempt` 代的總結（呼叫端已把總結列設為 pending）。
pub fn spawn_generation(state: AppState, id: Uuid, attempt: i32) {
    tokio::spawn(async move {
        if let Err(e) = generate(&state, id, attempt).await {
            // 不記錄資料庫錯誤訊息：約束錯誤可能帶出整列總結內容
            tracing::error!(class = %db_error_class(&e), "summary generation bookkeeping failed");
            // 盡力把這一代標記為失敗，避免永遠停在 pending
            let _ = sqlx::query(
                "UPDATE summaries SET status = 'failed', completed_at = now()
                 WHERE conversation_id = $1 AND attempt = $2 AND status = 'pending'",
            )
            .bind(id)
            .bind(attempt)
            .execute(&state.pool)
            .await;
        }
    });
}

async fn generate(state: &AppState, id: Uuid, attempt: i32) -> Result<(), sqlx::Error> {
    let (title, description): (String, String) =
        sqlx::query_as("SELECT title, description FROM conversations WHERE id = $1")
            .bind(id)
            .fetch_one(&state.pool)
            .await?;
    let turns: Vec<ChatTurn> = load_messages_for(state, id).await?;
    let req = AiRequest {
        system: build_summary_prompt(&title, &description),
        messages: vec![ChatTurn {
            role: ChatRole::User,
            content: format_transcript(&turns),
        }],
    };
    // 逾時或結果不完整時自動重試 1 次（S-03.5）；仍失敗就標記 failed
    let mut summary = None;
    for _ in 0..2 {
        let raw = collect(
            state.ai.as_ref(),
            req.clone(),
            state.config.ai_first_token_timeout,
            state.config.ai_total_timeout,
        )
        .await;
        if let Some(s) = raw.ok().and_then(|raw| parse_summary(&raw)) {
            summary = Some(s);
            break;
        }
    }
    match summary {
        Some(s) => {
            sqlx::query(
                "UPDATE summaries SET status = 'ready', stance = $2, reasons = $3,
                        turning_points = $4, rules_version = $5, completed_at = now()
                 WHERE conversation_id = $1 AND attempt = $6 AND status = 'pending'",
            )
            .bind(id)
            .bind(s.stance)
            .bind(s.reasons)
            .bind(s.turning_points)
            .bind(RULES_VERSION)
            .bind(attempt)
            .execute(&state.pool)
            .await?;
        }
        None => {
            sqlx::query(
                "UPDATE summaries SET status = 'failed', completed_at = now()
                 WHERE conversation_id = $1 AND attempt = $2 AND status = 'pending'",
            )
            .bind(id)
            .bind(attempt)
            .execute(&state.pool)
            .await?;
        }
    }
    Ok(())
}

async fn load_messages_for(state: &AppState, id: Uuid) -> Result<Vec<ChatTurn>, sqlx::Error> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT role, content FROM messages WHERE conversation_id = $1 ORDER BY seq",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(role, content)| ChatTurn {
            role: if role == "student" {
                ChatRole::User
            } else {
                ChatRole::Assistant
            },
            content,
        })
        .collect())
}

#[derive(Debug, Serialize, FromRow)]
pub struct SummaryView {
    pub status: String,
    pub stance: Option<String>,
    pub reasons: Option<String>,
    pub turning_points: Option<String>,
    pub completed_at: Option<DateTime<Utc>>,
}

const SUMMARY_VIEW: &str = "SELECT status, stance, reasons, turning_points, completed_at
                            FROM summaries WHERE conversation_id = $1";

async fn view(state: &AppState, id: Uuid) -> Result<SummaryView, ApiError> {
    sqlx::query_as(SUMMARY_VIEW)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(ApiError::not_found)
}

async fn end(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<SummaryView>), ApiError> {
    let conv = owned_conversation(&state, cu.user.id, id).await?;
    if conv.status == "ended" {
        // 重複結束不會再產生一次總結
        return Ok((StatusCode::OK, Json(view(&state, id).await?)));
    }
    if conv.turn_count == 0 {
        return Err(ApiError::conflict("empty_conversation", "還沒有討論內容"));
    }
    let mut tx = state.pool.begin().await?;
    let ended = end_conversation(&mut tx, id, state.config.generation_stale_secs()).await?;
    tx.commit().await?;
    if !ended {
        // 沒有結束成功：若對話其實已被結束（競態）就回傳目前狀態，否則是 AI 還在回覆
        let now = owned_conversation(&state, cu.user.id, id).await?;
        if now.status == "active" {
            return Err(ApiError::conflict("reply_in_progress", "AI 正在回覆中"));
        }
        return Ok((StatusCode::OK, Json(view(&state, id).await?)));
    }
    spawn_generation(state.clone(), id, 1);
    Ok((StatusCode::ACCEPTED, Json(view(&state, id).await?)))
}

async fn get_summary(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<SummaryView>, ApiError> {
    owned_conversation(&state, cu.user.id, id).await?;
    Ok(Json(view(&state, id).await?))
}

async fn retry(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<SummaryView>), ApiError> {
    owned_conversation(&state, cu.user.id, id).await?;
    // 只有失敗，或產生中卻已超過逾時上限（例如服務重啟）才能重試
    // 重試開新的一代（attempt + 1）；舊的背景任務之後寫入會因代數不符而被忽略
    let next: Option<(i32,)> = sqlx::query_as(
        "UPDATE summaries SET status = 'pending', attempt = attempt + 1, started_at = now(),
                completed_at = NULL
         WHERE conversation_id = $1
           AND (status = 'failed'
                OR (status = 'pending' AND started_at < now() - make_interval(secs => $2)))
         RETURNING attempt",
    )
    .bind(id)
    .bind(state.config.generation_stale_secs())
    .fetch_optional(&state.pool)
    .await?;
    let Some((attempt,)) = next else {
        // 沒有總結列代表對話還沒結束
        view(&state, id).await?;
        return Err(ApiError::conflict(
            "summary_not_retryable",
            "目前無法重新產生總結",
        ));
    };
    spawn_generation(state.clone(), id, attempt);
    Ok((StatusCode::ACCEPTED, Json(view(&state, id).await?)))
}

// ---- 教師檢視（F-26）----

#[derive(Debug, Serialize, FromRow)]
struct TeacherRow {
    conversation_id: Uuid,
    title: String,
    ended_at: Option<DateTime<Utc>>,
    student_id: Uuid,
    student_name: Option<String>,
    student_email: String,
    stance: Option<String>,
    reasons: Option<String>,
    turning_points: Option<String>,
}

/// 所有修課學生已完成對話的總結，每筆連到學生與該場對話；不含對話原文。
async fn teacher_list(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
) -> Result<Json<Vec<TeacherRow>>, ApiError> {
    let mut rows: Vec<TeacherRow> = sqlx::query_as(
        "SELECT s.conversation_id, c.title, c.ended_at,
                u.id AS student_id, u.display_name AS student_name, u.email AS student_email,
                s.stance, s.reasons, s.turning_points
         FROM summaries s
         JOIN conversations c ON c.id = s.conversation_id
         JOIN users u ON u.id = c.user_id
         WHERE s.status = 'ready' AND c.as_student
         ORDER BY c.ended_at DESC, s.conversation_id",
    )
    .fetch_all(&state.pool)
    .await?;
    // 管理者身分優先：總結屬敏感欄位，管理者一律看到遮蔽內容（S-02.2），
    // 即使同一帳號也是教師。
    if cu.roles.admin {
        for r in &mut rows {
            r.stance = Some(MASKED.into());
            r.reasons = Some(MASKED.into());
            r.turning_points = Some(MASKED.into());
        }
    }
    Ok(Json(rows))
}
