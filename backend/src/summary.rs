//! 結束對話與 AI 總結（F-08、F-26；規格見 S-03.3、S-02.1、S-02.2）。
//!
//! 對話結束後在背景產生總結；學生輪詢 `GET .../summary` 看狀態。AI 失敗或結果不完整時
//! 狀態為 `failed`，可用重試端點再試一次（F-08.4）。總結不可由學生修改。
//! 教師只看得到總結，看不到對話原文；管理者介面一律遮蔽（以 `message` 取代）。

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgConnection};
use uuid::Uuid;

use crate::{
    AppState,
    ai::{
        AiRequest, ChatRole, ChatTurn, RULES_VERSION, SCHOOLS, build_summary_prompt, collect,
        format_transcript, parse_summary,
    },
    auth::{RequireStudent, RequireTeacher},
    chat::owned_conversation,
    error::{ApiError, db_error_class},
    pagination::{self, Page, Pagination},
};

/// 6 維分數 → 存進資料庫與回給前端的 JSON 物件（鍵為學派）。
pub fn scores_json(scores: [u8; 6]) -> serde_json::Value {
    SCHOOLS
        .iter()
        .zip(scores)
        .map(|(k, v)| ((*k).to_string(), serde_json::Value::from(v)))
        .collect::<serde_json::Map<_, _>>()
        .into()
}

/// 管理者介面看到的固定替代字串（S-02.2）。
pub const MASKED: &str = "message";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/conversations/{id}/end", post(end))
        .route("/api/conversations/{id}/summary", get(get_summary))
        .route("/api/conversations/{id}/summary/retry", post(retry))
        .route("/api/summaries", get(list))
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
                        rules_version = $4, completed_at = now(),
                        framework = $6, framework_scores = $7, claim = $8
                 WHERE conversation_id = $1 AND attempt = $5 AND status = 'pending'",
            )
            .bind(id)
            .bind(s.stance)
            .bind(s.reasons)
            .bind(RULES_VERSION)
            .bind(attempt)
            .bind(s.framework)
            .bind(s.scores.map(scores_json))
            .bind(s.claim.as_deref())
            .execute(&state.pool)
            .await?;
            // 主張分組：盡力而為，失敗不影響總結
            if let Some(claim) = s.claim.as_deref() {
                crate::claims::assign_group(state, id, &title, claim).await;
            }
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

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub struct SummaryView {
    pub status: String,
    pub stance: Option<String>,
    pub reasons: Option<String>,
    /// 學生最終主張的一句短句（暫定）
    pub claim: Option<String>,
    /// 立場方向與倫理學派分類（暫定，供論點分布）；沒有分類時為 `None`
    pub framework: Option<String>,
    /// 6 個學派維度的分數（各 0–5）；沒有分數時為 `None`
    #[schema(value_type = Option<std::collections::BTreeMap<String, u8>>)]
    pub framework_scores: Option<serde_json::Value>,
    pub completed_at: Option<DateTime<Utc>>,
}

const SUMMARY_VIEW: &str = "SELECT status, stance, reasons, claim, framework,
                                   framework_scores, completed_at
                            FROM summaries WHERE conversation_id = $1";

async fn view(state: &AppState, id: Uuid) -> Result<SummaryView, ApiError> {
    sqlx::query_as(SUMMARY_VIEW)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(ApiError::not_found)
}

#[utoipa::path(
    post,
    path = "/api/conversations/{id}/end",
    tag = "conversations",
    summary = "結束對話並在背景產生總結",
    params(("id" = Uuid, Path, description = "對話 ID")),
    responses((status = 202, description = "已開始產生", body = SummaryView), (status = 200, description = "已結束，回傳目前總結", body = SummaryView), (status = 409, description = "empty_conversation / reply_in_progress", body = crate::error::ErrorBody))
)]
pub(crate) async fn end(
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

#[utoipa::path(
    get,
    path = "/api/conversations/{id}/summary",
    tag = "conversations",
    summary = "讀取總結（pending 時請輪詢）",
    params(("id" = Uuid, Path, description = "對話 ID")),
    responses((status = 200, description = "總結", body = SummaryView))
)]
pub(crate) async fn get_summary(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<SummaryView>, ApiError> {
    owned_conversation(&state, cu.user.id, id).await?;
    Ok(Json(view(&state, id).await?))
}

#[utoipa::path(
    post,
    path = "/api/conversations/{id}/summary/retry",
    tag = "conversations",
    summary = "總結失敗後重試",
    params(("id" = Uuid, Path, description = "對話 ID")),
    responses((status = 202, description = "已重新開始", body = SummaryView), (status = 409, description = "summary_not_retryable", body = crate::error::ErrorBody))
)]
pub(crate) async fn retry(
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

// ---- 教師與管理者檢視（F-26、#285）----

#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct SummaryRow {
    conversation_id: Uuid,
    title: String,
    ended_at: DateTime<Utc>,
    student_id: Uuid,
    student_name: Option<String>,
    student_email: String,
    stance: Option<String>,
    reasons: Option<String>,
    claim: Option<String>,
    framework: Option<String>,
    #[schema(value_type = Option<std::collections::BTreeMap<String, u8>>)]
    framework_scores: Option<serde_json::Value>,
}

/// 總結分頁；只給 OpenAPI 描述（泛型的 `Page<T>` 會把項目欄位展開，無法 `$ref`）。
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub(crate) struct SummaryPage {
    items: Vec<SummaryRow>,
    next_cursor: Option<String>,
}

/// 分頁參數加上選用篩選與搜尋。
#[derive(Deserialize)]
pub(crate) struct SummaryQuery {
    #[serde(flatten)]
    page: Pagination,
    /// 學生的使用者 ID（即列表項目的 `student_id`）；用 ID 而非電子郵件，網址與日誌不帶個資
    student: Option<String>,
    /// 對話複製來的題目標題，完全相符（與論點分布、主張分組以標題分題目一致）
    topic: Option<String>,
    /// 搜尋學生姓名、電子郵件或題目標題（不分大小寫的子字串），涵蓋所有頁
    q: Option<String>,
}

/// 搜尋字串長度上限（字元數）。
const MAX_SEARCH_CHARS: usize = 100;

/// 把搜尋字串轉成 `ILIKE ... ESCAPE '\'` 的樣式：跳脫萬用字元，前後加 `%`。
fn like_pattern(q: &str) -> String {
    let mut out = String::with_capacity(q.len() + 2);
    out.push('%');
    for c in q.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

struct Filters<'a> {
    student: Option<Uuid>,
    topic: Option<&'a str>,
    search: Option<String>,
}

impl SummaryQuery {
    fn filters(&self) -> Result<Filters<'_>, ApiError> {
        let invalid = || ApiError::bad_request("invalid_filter", "篩選條件不正確");
        let student = match self.student.as_deref() {
            None => None,
            Some(s) => Some(Uuid::parse_str(s).map_err(|_| invalid())?),
        };
        let topic = match self.topic.as_deref() {
            None => None,
            Some(t) if t.trim().is_empty() => return Err(invalid()),
            Some(t) => Some(t),
        };
        let search = match self.q.as_deref().map(str::trim) {
            None => None,
            Some(q) if q.is_empty() || q.chars().count() > MAX_SEARCH_CHARS => {
                return Err(invalid());
            }
            Some(q) => Some(like_pattern(q)),
        };
        Ok(Filters {
            student,
            topic,
            search,
        })
    }
}

/// 所有學生已完成對話的總結，每筆連到學生與該場對話；不含對話原文。
/// 教師與管理者可用；管理者看到的敏感欄位一律是 `message`（S-02.2）。
#[utoipa::path(
    get,
    path = "/api/summaries",
    tag = "summaries",
    summary = "所有學生已完成對話的總結（不含原文）",
    params(
        ("limit" = Option<u16>, Query, description = "1–200，預設 50"),
        ("cursor" = Option<String>, Query, description = "前頁 next_cursor"),
        ("student" = Option<Uuid>, Query, description = "只看這位學生（列表項目的 student_id）"),
        ("topic" = Option<String>, Query, description = "只看這個題目標題（完全相符）"),
        ("q" = Option<String>, Query, description = "搜尋學生姓名、電子郵件或題目標題（不分大小寫的子字串，最多 100 字元），涵蓋所有頁"),
    ),
    responses((status = 200, description = "總結分頁，依 ended_at DESC, conversation_id ASC", body = SummaryPage), (status = 400, description = "invalid_limit / invalid_cursor / invalid_filter", body = crate::error::ErrorBody))
)]
pub(crate) async fn list(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Query(query): Query<SummaryQuery>,
) -> Result<Json<Page<SummaryRow>>, ApiError> {
    let limit = query.page.limit()?;
    let cursor: Option<(DateTime<Utc>, Uuid)> = query.page.cursor("summaries")?;
    let Filters {
        student,
        topic,
        search,
    } = query.filters()?;
    let (after_time, after_id) = cursor.unzip();
    // 已完成的總結一定已結束（end_conversation 同時寫入 ended_at）；仍排除空值以保持 keyset 正確
    let mut rows: Vec<SummaryRow> = sqlx::query_as(
        "SELECT s.conversation_id, c.title, c.ended_at,
                u.id AS student_id, u.display_name AS student_name, u.email AS student_email,
                s.stance, s.reasons, s.claim, s.framework, s.framework_scores
         FROM summaries s
         JOIN conversations c ON c.id = s.conversation_id
         JOIN users u ON u.id = c.user_id
         WHERE s.status = 'ready' AND c.ended_at IS NOT NULL
           AND ($1::uuid IS NULL OR u.id = $1)
           AND ($2::text IS NULL OR c.title = $2)
           AND ($3::text IS NULL
                OR u.display_name ILIKE $3 ESCAPE '\\'
                OR u.email ILIKE $3 ESCAPE '\\'
                OR c.title ILIKE $3 ESCAPE '\\')
           AND ($4::timestamptz IS NULL
                OR c.ended_at < $4 OR (c.ended_at = $4 AND s.conversation_id > $5))
         ORDER BY c.ended_at DESC, s.conversation_id ASC
         LIMIT $6",
    )
    .bind(student)
    .bind(topic)
    .bind(search)
    .bind(after_time)
    .bind(after_id)
    .bind((limit + 1) as i64)
    .fetch_all(&state.pool)
    .await?;
    // 管理者身分優先：總結屬敏感欄位，管理者一律看到遮蔽內容（S-02.2），
    // 即使同一帳號也是教師。每一頁都在輸出前遮蔽。
    if cu.roles.admin {
        for r in &mut rows {
            r.stance = Some(MASKED.into());
            r.reasons = Some(MASKED.into());
            r.claim = Some(MASKED.into());
            // 分析結果同屬敏感欄位（S-02.2）
            r.framework = Some(MASKED.into());
            r.framework_scores = None;
        }
    }
    Ok(Json(pagination::page(
        rows,
        limit,
        "summaries",
        |row: &SummaryRow| (row.ended_at, row.conversation_id),
    )?))
}
