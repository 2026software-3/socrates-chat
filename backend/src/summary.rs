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
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgConnection};
use tracing::Instrument;
use uuid::Uuid;

use crate::{
    AppState,
    ai::{
        AiRequest, ChatRole, ChatTurn, build_summary_prompt, build_summary_translation_prompt,
        collect_background, format_transcript, parse_summary, parse_summary_translation,
        rules_version,
    },
    auth::{RequireStudent, RequireTeacher},
    chat::owned_conversation,
    classrooms::{ClassroomFilter, filtered_classroom},
    error::{ApiError, db_error_class},
    pagination::{self, Page, Pagination},
};

/// 管理者介面看到的固定替代字串（S-02.2）。
pub const MASKED: &str = "message";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/conversations/{id}/end", post(end))
        .route("/api/conversations/{id}/summary", get(get_summary))
        .route("/api/conversations/{id}/summary/retry", post(retry))
        .route("/api/summaries", get(list))
        .route("/api/summaries/{id}/translation", post(translate))
}

/// 結束對話並建立「產生中」的總結；回傳是否真的由這次呼叫結束。
///
/// 已結束，或 AI 正在回覆中（要等回覆完成，總結才包含學生看到的全部內容）時回傳 `false`。
pub async fn end_conversation(
    conn: &mut PgConnection,
    id: Uuid,
    stale_secs: f64,
) -> Result<bool, sqlx::Error> {
    let r = sqlx::query(concat!(
        "UPDATE conversations SET status = 'ended', ended_at = now()
         WHERE id = $1 AND status = 'active' AND ",
        generation_unclaimed!(),
    ))
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
    tokio::spawn(
        async move {
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
        }
        .in_current_span(),
    );
}

async fn generate(state: &AppState, id: Uuid, attempt: i32) -> Result<(), sqlx::Error> {
    let ((title, description, activity_id, language), turns): (
        (String, String, Option<Uuid>, String),
        _,
    ) = tokio::try_join!(
        sqlx::query_as(
            "SELECT title, description, activity_id, language FROM conversations WHERE id = $1"
        )
        .bind(id)
        .fetch_one(&state.pool),
        load_messages_for(state, id),
    )?;
    let req = AiRequest {
        system: build_summary_prompt(&title, &description, &language),
        messages: vec![ChatTurn {
            role: ChatRole::User,
            content: format_transcript(&turns, &language),
        }],
    };
    // 逾時或結果不完整時自動重試 1 次（S-03.5）；仍失敗就標記 failed
    let mut summary = None;
    for _ in 0..2 {
        let raw = collect_background(
            state.ai.as_ref(),
            req.clone(),
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
                        rules_version = $4, completed_at = now(), claim = $6
                 WHERE conversation_id = $1 AND attempt = $5 AND status = 'pending'",
            )
            .bind(id)
            .bind(s.stance)
            .bind(s.reasons)
            .bind(rules_version(&language))
            .bind(attempt)
            .bind(s.claim.as_deref())
            .execute(&state.pool)
            .await?;
            // 主張分組以活動為單位（S-04.7）：個人練習沒有活動，不分組。盡力而為，失敗不影響總結
            if let (Some(claim), Some(activity)) = (s.claim.as_deref(), activity_id) {
                crate::claims::assign_group(state, id, activity, &title, claim).await;
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
            role: ChatRole::from_db_role(&role),
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
    pub completed_at: Option<DateTime<Utc>>,
}

const SUMMARY_VIEW: &str = "SELECT status, stance, reasons, claim, completed_at
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
    responses((status = 202, description = "已開始產生", body = SummaryView), (status = 200, description = "已結束，回傳目前總結", body = SummaryView), (status = 409, description = "classroom_archived / empty_conversation / reply_in_progress", body = crate::error::ErrorBody))
)]
pub(crate) async fn end(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<SummaryView>), ApiError> {
    // 封存檢查與結束寫入在同一個交易內
    let mut tx = state.pool.begin().await?;
    let conv = crate::classrooms::writable_conversation(&mut tx, cu.user.id, id).await?;
    if conv.status == "ended" {
        // 重複結束不會再產生一次總結
        return Ok((StatusCode::OK, Json(view(&state, id).await?)));
    }
    if conv.turn_count == 0 {
        return Err(ApiError::conflict("empty_conversation", "還沒有討論內容"));
    }
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
    // Capture the accepted response before the background task can finish; a fast AI provider
    // must not turn the initial 202 body into `ready` nondeterministically.
    let pending = view(&state, id).await?;
    spawn_generation(state.clone(), id, 1);
    Ok((StatusCode::ACCEPTED, Json(pending)))
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
    responses((status = 202, description = "已重新開始", body = SummaryView), (status = 409, description = "classroom_archived / summary_not_retryable", body = crate::error::ErrorBody))
)]
pub(crate) async fn retry(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<SummaryView>), ApiError> {
    // 封存檢查與重試寫入在同一個交易內
    let mut tx = state.pool.begin().await?;
    crate::classrooms::writable_conversation(&mut tx, cu.user.id, id).await?;
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
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
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
    /// 對話所屬的教室
    classroom_id: Uuid,
    title: String,
    ended_at: DateTime<Utc>,
    /// 排序與游標用，不輸出
    #[serde(skip)]
    completed_at: DateTime<Utc>,
    student_id: Uuid,
    student_name: Option<String>,
    student_email: Option<String>,
    /// 對話建立時快照的原文語言。
    language: String,
    stance: Option<String>,
    reasons: Option<String>,
    claim: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct SummaryTranslation {
    language: String,
    stance: String,
    reasons: String,
}

/// 教師明確要求時，僅把自己教室內既有總結的兩個欄位翻成教師目前的介面語言。
/// 不讀取也不送出對話原文、學生姓名或電子郵件；管理者介面維持敏感欄位遮蔽，不提供翻譯。
#[utoipa::path(
    post,
    path = "/api/summaries/{id}/translation",
    tag = "summaries",
    summary = "按需翻譯一筆學生總結（不含對話原文）",
    params(("id" = Uuid, Path, description = "對話 ID")),
    responses(
        (status = 200, description = "未保存的翻譯結果", body = SummaryTranslation),
        (status = 429, description = "翻譯太頻繁（too_many_translations）", body = crate::error::ErrorBody),
        (status = 502, description = "AI 無法翻譯，或譯文過長（ai_unavailable）", body = crate::error::ErrorBody),
        crate::error::CommonErrors
    )
)]
pub(crate) async fn translate(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<SummaryTranslation>, ApiError> {
    if cu.roles.admin {
        return Err(ApiError::forbidden("forbidden", "沒有權限"));
    }
    let source: Option<(String, String, String)> = sqlx::query_as(
        "SELECT s.stance, s.reasons, c.language
         FROM summaries s
         JOIN conversations c ON c.id = s.conversation_id
         JOIN classrooms cl ON cl.id = c.classroom_id AND cl.created_by = $2
         WHERE s.conversation_id = $1 AND s.status = 'ready'",
    )
    .bind(id)
    .bind(cu.user.id)
    .fetch_optional(&state.pool)
    .await?;
    let Some((stance, reasons, source_language)) = source else {
        return Err(ApiError::not_found());
    };
    let language = cu.user.language;
    if language == source_language {
        return Ok(Json(SummaryTranslation {
            language,
            stance,
            reasons,
        }));
    }
    // 翻譯不保存也不快取，每次都是一次 AI 呼叫：每位教師限量
    let Some(slot) = state.translation_limit.try_acquire(cu.user.id) else {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_translations",
            "翻譯太頻繁，請稍後再試",
        ));
    };
    let req = AiRequest {
        system: build_summary_translation_prompt(&language),
        messages: vec![ChatTurn {
            role: ChatRole::User,
            content: serde_json::json!({"stance": stance, "reasons": reasons}).to_string(),
        }],
    };
    let translated = collect_background(state.ai.as_ref(), req, state.config.ai_total_timeout)
        .await
        .ok()
        .and_then(|raw| parse_summary_translation(&raw));
    let Some(translated) = translated else {
        // 沒有得到譯文就不扣額度：AI 當機時的重試不該把教師鎖在外面
        state.translation_limit.refund(cu.user.id, slot);
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "ai_unavailable",
            "AI 暫時無法回覆",
        ));
    };
    Ok(Json(SummaryTranslation {
        language,
        stance: translated.stance,
        reasons: translated.reasons,
    }))
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
    #[serde(flatten)]
    classroom: ClassroomFilter,
    /// 學生的使用者 ID（即列表項目的 `student_id`）；用 ID 而非電子郵件，網址與日誌不帶個資
    student: Option<String>,
    /// 討論活動的 ID；只看這個活動的對話
    activity: Option<String>,
    /// 對話複製來的題目標題，完全相符但不分大小寫（以標題分題目，與論點分布、主張分組一致）
    topic: Option<String>,
    /// 搜尋學生姓名、電子郵件或題目標題（不分大小寫的子字串），涵蓋所有頁
    q: Option<String>,
}

/// 游標種類；排序鍵改為 completed_at 後換名，依 ended_at 產生的舊游標回 `invalid_cursor`。
const CURSOR_KIND: &str = "summaries_by_completion";

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
    activity: Option<Uuid>,
    topic: Option<&'a str>,
    search: Option<String>,
    /// 篩選條件的指紋；放進游標，換條件後沿用舊游標會被拒絕。
    fingerprint: String,
}

/// 篩選條件（正規化後，含教室）的雜湊，不可逆，所以游標裡不會帶出姓名或電子郵件。
fn fingerprint(
    classroom: Option<Uuid>,
    student: Option<Uuid>,
    activity: Option<Uuid>,
    topic: Option<&str>,
    q: Option<&str>,
) -> String {
    let normalized = (
        classroom,
        student,
        activity,
        topic.map(str::to_lowercase),
        q.map(str::to_lowercase),
    );
    let bytes = serde_json::to_vec(&normalized).unwrap_or_default();
    URL_SAFE_NO_PAD.encode(&Sha256::digest(bytes)[..8])
}

impl SummaryQuery {
    fn filters(&self, classroom: Option<Uuid>) -> Result<Filters<'_>, ApiError> {
        let invalid = || ApiError::bad_request("invalid_filter", "篩選條件不正確");
        let student = self
            .student
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()
            .map_err(|_| invalid())?;
        let activity = match self.activity.as_deref() {
            None => None,
            Some(a) => Some(Uuid::parse_str(a).map_err(|_| invalid())?),
        };
        let topic = match self.topic.as_deref() {
            None => None,
            Some(t) if t.trim().is_empty() => return Err(invalid()),
            Some(t) => Some(t),
        };
        let q = match self.q.as_deref().map(str::trim) {
            None => None,
            Some(q) if q.is_empty() || q.chars().count() > MAX_SEARCH_CHARS => {
                return Err(invalid());
            }
            Some(q) => Some(q),
        };
        Ok(Filters {
            student,
            activity,
            topic,
            search: q.map(like_pattern),
            fingerprint: fingerprint(classroom, student, activity, topic, q),
        })
    }
}

/// 自己管理的教室內，學生已完成活動對話的總結（管理者為全部教室），每筆連到學生與該場對話；不含對話原文。
/// 個人練習（不屬於任何教室）一律不在其中（S-02.1、S-14）。管理者看到的敏感欄位一律是 `message`（S-02.2）。
#[utoipa::path(
    get,
    path = "/api/summaries",
    tag = "summaries",
    summary = "教室內學生已完成對話的總結（不含原文；不含個人練習）",
    params(
        ("limit" = Option<u16>, Query, description = "1–200，預設 50"),
        ("cursor" = Option<String>, Query, description = "前頁 next_cursor；只對產生它時的 classroom_id／student／activity／topic／q 有效，換條件要從第一頁開始"),
        ("classroom_id" = Option<Uuid>, Query, description = "只看這間教室；不是自己管理的教室回 404"),
        ("student" = Option<Uuid>, Query, description = "只看這位學生（列表項目的 student_id）"),
        ("activity" = Option<Uuid>, Query, description = "只看這個討論活動的對話（活動 ID）"),
        ("topic" = Option<String>, Query, description = "只看這個題目標題（完全相符，不分大小寫）"),
        ("q" = Option<String>, Query, description = "搜尋學生姓名、電子郵件或題目標題（不分大小寫的子字串，最多 100 字元），涵蓋所有頁"),
    ),
    responses((status = 200, description = "總結分頁，依總結完成時間新到舊（completed_at DESC, conversation_id ASC）", body = SummaryPage), (status = 400, description = "invalid_limit / invalid_cursor / invalid_filter", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn list(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Query(query): Query<SummaryQuery>,
) -> Result<Json<Page<SummaryRow>>, ApiError> {
    let limit = query.page.limit()?;
    let classroom = filtered_classroom(&state, &cu, &query.classroom).await?;
    let Filters {
        student,
        activity,
        topic,
        search,
        fingerprint,
    } = query.filters(classroom)?;
    let cursor: Option<(DateTime<Utc>, Uuid, String)> = query.page.cursor(CURSOR_KIND)?;
    // 游標只對產生它時的篩選條件有效；換條件要從第一頁開始
    if cursor.as_ref().is_some_and(|c| c.2 != fingerprint) {
        return Err(ApiError::bad_request("invalid_cursor", "分頁游標不正確"));
    }
    let (after_time, after_id) = cursor.map(|c| (c.0, c.1)).unzip();
    // 依總結完成時間排序：翻頁期間才完成的總結一定排在第一頁最上面，不會插進已翻過的位置。
    // 內連接教室：classroom_id 為空的個人練習無法被比對到
    let mut rows: Vec<SummaryRow> = sqlx::query_as(
        "SELECT s.conversation_id, c.classroom_id, c.title, c.ended_at, s.completed_at,
                u.id AS student_id, u.display_name AS student_name, u.email AS student_email,
                c.language,
                s.stance, s.reasons, s.claim
         FROM summaries s
         JOIN conversations c ON c.id = s.conversation_id
         JOIN classrooms cl ON cl.id = c.classroom_id AND ($1 OR cl.created_by = $2)
         JOIN users u ON u.id = c.user_id
         WHERE s.status = 'ready' AND c.ended_at IS NOT NULL
           AND ($3::uuid IS NULL OR c.classroom_id = $3)
           AND ($4::uuid IS NULL OR u.id = $4)
           AND ($5::text IS NULL OR lower(c.title) = lower($5))
           AND ($6::text IS NULL
                OR u.display_name ILIKE $6 ESCAPE '\\'
                OR u.email ILIKE $6 ESCAPE '\\'
                OR c.title ILIKE $6 ESCAPE '\\')
           AND ($7::timestamptz IS NULL
                OR s.completed_at < $7 OR (s.completed_at = $7 AND s.conversation_id > $8))
           AND ($9::uuid IS NULL OR c.activity_id = $9)
         ORDER BY s.completed_at DESC, s.conversation_id ASC
         LIMIT $10",
    )
    .bind(cu.roles.admin)
    .bind(cu.user.id)
    .bind(classroom)
    .bind(student)
    .bind(topic)
    .bind(search)
    .bind(after_time)
    .bind(after_id)
    .bind(activity)
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
        }
    }
    Ok(Json(pagination::page(
        rows,
        limit,
        CURSOR_KIND,
        |row: &SummaryRow| (row.completed_at, row.conversation_id, fingerprint),
    )?))
}
