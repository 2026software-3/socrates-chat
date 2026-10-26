//! 教師清單與教室的修課名單（F-01.2、F-25、F-28.2；規格見 S-01.3、S-01.4、S-14）。

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::{
    AppState,
    accounts::audit,
    auth::{RequireAdmin, RequireTeacher, hash_blocking},
    auth_store,
    classrooms::visible_classroom,
    error::ApiError,
    pagination::{self, Page, Pagination},
    password, roster_store,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/teachers", get(list_teachers).post(add_teacher))
        .route("/api/admin/teachers/{email}", delete(remove_teacher))
        .route("/api/classrooms/{id}/roster", get(list_roster))
        .route("/api/classrooms/{id}/roster/import", post(import_roster))
        .route(
            "/api/classrooms/{id}/roster/{email}",
            delete(remove_from_roster),
        )
}

/// 去空白並轉小寫；格式不合回傳 `None`。只做基本檢查，真正的驗證由 Google 登入完成。
pub fn normalize_email(raw: &str) -> Option<String> {
    let e = raw.trim().trim_matches('"').trim().to_lowercase();
    let (local, domain) = e.split_once('@')?;
    let ok = !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains('@')
        && !e.chars().any(char::is_whitespace);
    ok.then_some(e)
}

// ---- 教師（管理者操作）----

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct EmailBody {
    email: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct EmailList {
    emails: Vec<String>,
}

const LIST_TEACHERS: &str = "SELECT email FROM teachers ORDER BY email";
const DELETE_TEACHER: &str = "DELETE FROM teachers WHERE email = lower($1)";

async fn list_emails(pool: &PgPool, sql: &'static str) -> Result<Vec<String>, ApiError> {
    let rows: Vec<(String,)> = sqlx::query_as(sql).fetch_all(pool).await?;
    Ok(rows.into_iter().map(|(e,)| e).collect())
}

#[utoipa::path(
    get,
    path = "/api/admin/teachers",
    tag = "admin",
    summary = "列出教師",
    responses((status = 200, description = "教師信箱", body = EmailList))
)]
pub(crate) async fn list_teachers(
    _: RequireAdmin,
    State(state): State<AppState>,
) -> Result<Json<EmailList>, ApiError> {
    Ok(Json(EmailList {
        emails: list_emails(&state.pool, LIST_TEACHERS).await?,
    }))
}

#[utoipa::path(
    post,
    path = "/api/admin/teachers",
    tag = "admin",
    summary = "新增教師（重複新增也回 204）",
    request_body = EmailBody,
    responses((status = 204, description = "完成"), (status = 400, description = "invalid_email", body = crate::error::ErrorBody))
)]
pub(crate) async fn add_teacher(
    RequireAdmin(cu): RequireAdmin,
    State(state): State<AppState>,
    Json(body): Json<EmailBody>,
) -> Result<StatusCode, ApiError> {
    let email = normalize_email(&body.email)
        .ok_or(ApiError::bad_request("invalid_email", "電子郵件格式不正確"))?;
    let mut tx = state.pool.begin().await?;
    let r = sqlx::query("INSERT INTO teachers (email) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(&email)
        .execute(&mut *tx)
        .await?;
    // 重複新增沒有改變任何事，不重複記錄
    if r.rows_affected() > 0 {
        audit(&mut *tx, cu.user.id, "teacher_added", &email, None).await?;
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete,
    path = "/api/admin/teachers/{email}",
    tag = "admin",
    summary = "移除教師身分",
    params(("email" = String, Path, description = "電子郵件")),
    responses((status = 204, description = "已移除"), (status = 404, description = "not_found", body = crate::error::ErrorBody))
)]
pub(crate) async fn remove_teacher(
    RequireAdmin(cu): RequireAdmin,
    State(state): State<AppState>,
    Path(email): Path<String>,
) -> Result<StatusCode, ApiError> {
    delete_email(
        &state.pool,
        DELETE_TEACHER,
        &email,
        cu.user.id,
        "teacher_removed",
    )
    .await
}

async fn delete_email(
    pool: &PgPool,
    sql: &'static str,
    email: &str,
    actor: uuid::Uuid,
    action: &str,
) -> Result<StatusCode, ApiError> {
    let mut tx = pool.begin().await?;
    let r = sqlx::query(sql).bind(email).execute(&mut *tx).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    audit(&mut *tx, actor, action, &email.to_lowercase(), None).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- 教室的修課名單（教室的建立者或管理者操作；他人的教室與不存在的教室一律 404）----

/// 名單上的一筆；以物件包裝，之後要加欄位不必再改契約。
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct RosterEntry {
    email: String,
}

/// 名單分頁；只給 OpenAPI 描述（泛型的 `Page<T>` 會把項目欄位展開，無法 `$ref`）。
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub(crate) struct RosterPage {
    items: Vec<RosterEntry>,
    next_cursor: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/classrooms/{id}/roster",
    tag = "roster",
    summary = "教室的修課名單",
    params(
        ("id" = Uuid, Path, description = "教室 ID"),
        ("limit" = Option<u16>, Query, description = "1–200，預設 50"),
        ("cursor" = Option<String>, Query, description = "前頁 next_cursor"),
    ),
    responses((status = 200, description = "名單分頁，依 email 排序", body = RosterPage), (status = 400, description = "invalid_limit / invalid_cursor", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn list_roster(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
    Query(query): Query<Pagination>,
) -> Result<Json<Page<RosterEntry>>, ApiError> {
    let limit = query.limit()?;
    let after: Option<String> = query.cursor("roster")?;
    let classroom = visible_classroom(&state, &cu, &raw_id).await?;
    let rows = roster_store::list_emails(
        &state.pool,
        classroom.id,
        after.as_deref(),
        (limit + 1) as i64,
    )
    .await?;
    let rows = rows
        .into_iter()
        .map(|email| RosterEntry { email })
        .collect();
    Ok(Json(pagination::page(rows, limit, "roster", |row| {
        row.email.clone()
    })?))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct ImportBody {
    text: String,
}

#[derive(Serialize, Debug, PartialEq, utoipa::ToSchema)]
pub struct InvalidLine {
    pub line: usize,
    pub value: String,
}

#[derive(Serialize, Debug, Default, PartialEq)]
pub struct ParsedRoster {
    pub emails: Vec<String>,
    pub invalid: Vec<InvalidLine>,
}

/// 解析貼上或上傳的名單：每行一個電子郵件，可有 `email` 標題列；CSV 取第一欄。
pub fn parse_roster(text: &str) -> ParsedRoster {
    let mut out = ParsedRoster::default();
    let mut seen_header_slot = true;
    for (i, raw) in text.lines().enumerate() {
        let first = raw.split(',').next().unwrap_or_default().trim();
        if first.is_empty() {
            continue;
        }
        if seen_header_slot && first.trim_matches('"').eq_ignore_ascii_case("email") {
            seen_header_slot = false;
            continue;
        }
        seen_header_slot = false;
        match normalize_email(first) {
            Some(e) if !out.emails.contains(&e) => out.emails.push(e),
            Some(_) => {}
            None => out.invalid.push(InvalidLine {
                line: i + 1,
                value: first.to_string(),
            }),
        }
    }
    out
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ImportResult {
    added: usize,
    existing: usize,
    invalid: Vec<InvalidLine>,
    /// 這次新建立的內建帳號與臨時密碼（只在這次回應出現）；信箱已有帳號的不在其中。
    credentials: Vec<Credential>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct Credential {
    email: String,
    temporary_password: String,
}

/// 信箱是否屬於教師或管理者（教師清單、`ADMIN_EMAILS` 或已是管理者的使用者）。
pub(crate) async fn is_staff_email(
    state: &AppState,
    conn: &mut sqlx::PgConnection,
    email: &str,
) -> Result<bool, ApiError> {
    if state.config.admin_emails.iter().any(|e| e == email) {
        return Ok(true);
    }
    let (staff,): (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM teachers WHERE email = $1)
             OR EXISTS (SELECT 1 FROM users WHERE email = $1 AND is_admin)",
    )
    .bind(email)
    .fetch_one(conn)
    .await?;
    Ok(staff)
}

#[utoipa::path(
    post,
    path = "/api/classrooms/{id}/roster/import",
    tag = "roster",
    summary = "匯入教室的修課名單（只新增）",
    params(("id" = Uuid, Path, description = "教室 ID")),
    request_body = ImportBody,
    responses((status = 200, description = "匯入結果", body = ImportResult), (status = 409, description = "classroom_archived", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn import_roster(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
    Json(body): Json<ImportBody>,
) -> Result<Json<ImportResult>, ApiError> {
    let classroom = visible_classroom(&state, &cu, &raw_id).await?;
    let parsed = parse_roster(&body.text);
    let mut added = 0;
    let mut credentials = Vec::new();
    let mut tx = state.pool.begin().await?;
    // 封存檢查與寫入同一個交易：教室列的共享鎖持續到提交，封存不會插在中間
    crate::classrooms::ensure_writable(&mut *tx, classroom.id).await?;
    // 整批名單、帳號與稽核一起提交，避免失敗時遺失一次性的臨時密碼。
    // 匯入只新增，不移除名單上原有的人
    for email in &parsed.emails {
        if !roster_store::add(&mut tx, classroom.id, email).await? {
            continue;
        }
        added += 1;
        audit(&mut *tx, cu.user.id, "roster_added", email, None).await?;
        // 只有管理者匯入時才建立內建帳號並取得臨時密碼：知道臨時密碼等於能用該學生的身分登入，
        // 教師不能看到對話原文（S-02.1），所以教師匯入只加名單，學生用 Google 登入或請管理者重設密碼。
        // 教師與管理者的信箱也不在這裡建帳號，避免有人搶先替他們建好帳號。
        if !cu.roles.admin || is_staff_email(&state, &mut tx, email).await? {
            continue;
        }
        let temporary_password = password::generate_temporary();
        let hash = hash_blocking(temporary_password.clone()).await?;
        if auth_store::create_account_if_absent(&mut *tx, email, &hash).await? {
            credentials.push(Credential {
                email: email.clone(),
                temporary_password,
            });
        }
    }
    tx.commit().await?;
    Ok(Json(ImportResult {
        added,
        existing: parsed.emails.len() - added,
        invalid: parsed.invalid,
        credentials,
    }))
}

#[utoipa::path(
    delete,
    path = "/api/classrooms/{id}/roster/{email}",
    tag = "roster",
    summary = "移出教室名單（資料保留；只影響這間教室）",
    params(
        ("id" = Uuid, Path, description = "教室 ID"),
        ("email" = String, Path, description = "電子郵件")
    ),
    responses((status = 204, description = "已移出"), (status = 404, description = "not_found：教室不存在或不屬於你，或此人不在這間教室的名單內", body = crate::error::ErrorBody), (status = 409, description = "classroom_archived", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn remove_from_roster(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path((raw_id, email)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let classroom = visible_classroom(&state, &cu, &raw_id).await?;
    let mut tx = state.pool.begin().await?;
    crate::classrooms::ensure_writable(&mut *tx, classroom.id).await?;
    if !roster_store::remove(&mut tx, classroom.id, &email).await? {
        return Err(ApiError::not_found());
    }
    audit(
        &mut *tx,
        cu.user.id,
        "roster_removed",
        &email.to_lowercase(),
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
