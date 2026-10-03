//! 教師清單與修課名單（F-01.2、F-25；規格見 S-01.3、S-01.4）。

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::{
    AppState,
    auth::{RequireAdmin, RequireTeacher, hash_blocking},
    auth_store,
    error::ApiError,
    password,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/teachers", get(list_teachers).post(add_teacher))
        .route("/api/admin/teachers/{email}", delete(remove_teacher))
        .route("/api/roster", get(list_roster))
        .route("/api/roster/import", post(import_roster))
        .route("/api/roster/{email}", delete(remove_from_roster))
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

#[derive(Deserialize)]
struct EmailBody {
    email: String,
}

#[derive(Serialize)]
struct EmailList {
    emails: Vec<String>,
}

const LIST_TEACHERS: &str = "SELECT email FROM teachers ORDER BY email";
const LIST_ROSTER: &str = "SELECT email FROM enrollments ORDER BY email";
const DELETE_TEACHER: &str = "DELETE FROM teachers WHERE email = lower($1)";
const DELETE_ROSTER: &str = "DELETE FROM enrollments WHERE email = lower($1)";

async fn list_emails(pool: &PgPool, sql: &'static str) -> Result<Vec<String>, ApiError> {
    let rows: Vec<(String,)> = sqlx::query_as(sql).fetch_all(pool).await?;
    Ok(rows.into_iter().map(|(e,)| e).collect())
}

async fn list_teachers(
    _: RequireAdmin,
    State(state): State<AppState>,
) -> Result<Json<EmailList>, ApiError> {
    Ok(Json(EmailList {
        emails: list_emails(&state.pool, LIST_TEACHERS).await?,
    }))
}

async fn add_teacher(
    _: RequireAdmin,
    State(state): State<AppState>,
    Json(body): Json<EmailBody>,
) -> Result<StatusCode, ApiError> {
    let email = normalize_email(&body.email)
        .ok_or(ApiError::bad_request("invalid_email", "電子郵件格式不正確"))?;
    sqlx::query("INSERT INTO teachers (email) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(email)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_teacher(
    _: RequireAdmin,
    State(state): State<AppState>,
    Path(email): Path<String>,
) -> Result<StatusCode, ApiError> {
    delete_email(&state.pool, DELETE_TEACHER, &email).await
}

async fn delete_email(
    pool: &PgPool,
    sql: &'static str,
    email: &str,
) -> Result<StatusCode, ApiError> {
    let r = sqlx::query(sql).bind(email).execute(pool).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---- 修課名單（教師或管理者操作）----

async fn list_roster(
    _: RequireTeacher,
    State(state): State<AppState>,
) -> Result<Json<EmailList>, ApiError> {
    Ok(Json(EmailList {
        emails: list_emails(&state.pool, LIST_ROSTER).await?,
    }))
}

#[derive(Deserialize)]
struct ImportBody {
    text: String,
}

#[derive(Serialize, Debug, PartialEq)]
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

#[derive(Serialize)]
struct ImportResult {
    added: usize,
    existing: usize,
    invalid: Vec<InvalidLine>,
    /// 這次新建立的內建帳號與臨時密碼（只在這次回應出現）；信箱已有帳號的不在其中。
    credentials: Vec<Credential>,
}

#[derive(Serialize)]
struct Credential {
    email: String,
    temporary_password: String,
}

async fn import_roster(
    _: RequireTeacher,
    State(state): State<AppState>,
    Json(body): Json<ImportBody>,
) -> Result<Json<ImportResult>, ApiError> {
    let parsed = parse_roster(&body.text);
    let mut added = 0;
    let mut credentials = Vec::new();
    // 匯入只新增，不移除名單上原有的人
    for email in &parsed.emails {
        let r = sqlx::query("INSERT INTO enrollments (email) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(email)
            .execute(&state.pool)
            .await?;
        if r.rows_affected() == 0 {
            continue;
        }
        added += 1;
        // 新加入的學生同時建立內建帳號（自動產生臨時密碼）；已有帳號的不動
        let temporary_password = password::generate_temporary();
        let hash = hash_blocking(temporary_password.clone()).await?;
        if auth_store::create_account_if_absent(&state.pool, email, &hash).await? {
            credentials.push(Credential {
                email: email.clone(),
                temporary_password,
            });
        }
    }
    Ok(Json(ImportResult {
        added,
        existing: parsed.emails.len() - added,
        invalid: parsed.invalid,
        credentials,
    }))
}

async fn remove_from_roster(
    _: RequireTeacher,
    State(state): State<AppState>,
    Path(email): Path<String>,
) -> Result<StatusCode, ApiError> {
    delete_email(&state.pool, DELETE_ROSTER, &email).await
}
