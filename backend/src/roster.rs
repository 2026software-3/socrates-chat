//! 教師清單與教室的修課名單（F-01.2、F-25、F-28.2；規格見 S-01.3、S-01.4、S-14）。

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::{
    AppState,
    accounts::audit,
    auth::{RequireAdmin, RequireTeacher, hash_blocking},
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

/// 名單分頁；只給 OpenAPI 描述（泛型的 `Page<T>` 會把項目欄位展開，無法 `$ref`）。
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
pub(crate) struct RosterPage {
    items: Vec<roster_store::RosterEntry>,
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
    responses((status = 200, description = "名單分頁，依帳號 UUID 排序", body = RosterPage), (status = 400, description = "invalid_limit / invalid_cursor", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn list_roster(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
    Query(query): Query<Pagination>,
) -> Result<Json<Page<roster_store::RosterEntry>>, ApiError> {
    let limit = query.limit()?;
    let after: Option<uuid::Uuid> = query.cursor("roster")?;
    let classroom = visible_classroom(&state, &cu, &raw_id).await?;
    let rows =
        roster_store::list_roster(&state.pool, classroom.id, after, (limit + 1) as i64).await?;
    Ok(Json(pagination::page(rows, limit, "roster", |row| row.id)?))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct ImportBody {
    text: String,
}

#[derive(Debug, PartialEq)]
pub struct StudentImportRow {
    pub line: usize,
    pub name: String,
    pub student_id: String,
    pub email: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, utoipa::ToSchema)]
pub struct StudentImportError {
    pub line: usize,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct ParsedStudentRoster {
    pub students: Vec<StudentImportRow>,
    pub invalid: Vec<StudentImportError>,
}

/// 解析單行 CSV，支援引號內逗號與雙引號跳脫；不接受跨行欄位。
fn csv_fields(line: &str) -> Option<Vec<String>> {
    let mut fields = Vec::new();
    let mut value = String::new();
    let mut chars = line.chars().peekable();
    let mut quoted = false;
    let mut closed = false;
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    value.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                value.push(c);
            }
        } else if c == ',' {
            fields.push(value.trim().to_owned());
            value.clear();
            closed = false;
        } else if c == '"' && !closed && value.trim().is_empty() {
            value.clear();
            quoted = true;
        } else if c == '"' || (closed && !c.is_whitespace()) {
            return None;
        } else {
            value.push(c);
        }
    }
    if quoted {
        return None;
    }
    fields.push(value.trim().to_owned());
    Some(fields)
}

/// 學號為 8 位數字加 1 個英文字母（與資料庫 CHECK 一致，大小寫皆收）。
fn is_valid_student_id(id: &str) -> bool {
    id.len() == 9
        && id.as_bytes()[..8].iter().all(u8::is_ascii_digit)
        && id.as_bytes()[8].is_ascii_alphabetic()
}

/// 每行依序為姓名、學號及選填信箱；錯誤不回顯個資。
pub fn parse_student_roster(text: &str) -> ParsedStudentRoster {
    let mut result = ParsedStudentRoster::default();
    for (index, line) in text.lines().enumerate() {
        let Some(fields) = csv_fields(line) else {
            result.invalid.push(StudentImportError {
                line: index + 1,
                reason: "invalid_format".to_string(),
            });
            continue;
        };
        if fields.len() != 3 || fields[0].is_empty() || fields[1].is_empty() {
            result.invalid.push(StudentImportError {
                line: index + 1,
                reason: "missing_fields".to_string(),
            });
            continue;
        }
        let email = if fields[2].is_empty() {
            Some(None)
        } else {
            normalize_email(&fields[2]).map(Some)
        };
        let valid = fields[0].chars().count() <= 30
            && !fields[0].contains('\0')
            && is_valid_student_id(&fields[1]);
        let (true, Some(email)) = (valid, email) else {
            result.invalid.push(StudentImportError {
                line: index + 1,
                reason: "invalid_format".to_string(),
            });
            continue;
        };
        result.students.push(StudentImportRow {
            line: index + 1,
            name: fields[0].to_owned(),
            student_id: fields[1].to_ascii_uppercase(),
            email,
        });
    }
    result
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ImportResult {
    added: usize,
    existing: usize,
    errors: usize,
    invalid: Vec<StudentImportError>,
    /// 這次新建立的內建帳號與臨時密碼（只在這次回應出現）；信箱已有帳號的不在其中。
    credentials: Vec<Credential>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct Credential {
    student_id: String,
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

/// 匯入交易共用的 advisory lock 鍵。
const IMPORT_LOCK_KEY: i64 = 393;

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
) -> Result<([(header::HeaderName, &'static str); 1], Json<ImportResult>), ApiError> {
    let classroom = visible_classroom(&state, &cu, &raw_id).await?;
    let parsed = parse_student_roster(&body.text);
    let mut added = 0;
    let mut existing = 0;
    let mut invalid = parsed.invalid;
    let mut credentials = Vec::new();
    let mut tx = state.pool.begin().await?;
    // 封存檢查與寫入同一個交易：教室列的共享鎖持續到提交，封存不會插在中間
    crate::classrooms::ensure_writable(&mut *tx, classroom.id).await?;
    // 整批名單、帳號與稽核一起提交，避免失敗時遺失一次性的臨時密碼。
    // 匯入只新增，不移除名單上原有的人
    // 匯入交易序列化，避免兩個教室同時替相同學號或信箱建立帳號。
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(IMPORT_LOCK_KEY)
        .execute(&mut *tx)
        .await?;
    for row in &parsed.students {
        if let Some(email) = row.email.as_deref()
            && is_staff_email(&state, &mut tx, email).await?
        {
            invalid.push(StudentImportError {
                line: row.line,
                reason: "email_conflict".into(),
            });
            continue;
        }
        let accounts =
            roster_store::find_import_account(&mut tx, &row.student_id, row.email.as_deref())
                .await?;
        let id_conflict = accounts.iter().any(|account| {
            account.student_id.as_deref() == Some(&row.student_id)
                && account
                    .email
                    .as_deref()
                    .zip(row.email.as_deref())
                    .is_some_and(|(a, b)| a != b)
        });
        let email_conflict = accounts.len() > 1
            || accounts.iter().any(|account| {
                account
                    .student_id
                    .as_deref()
                    .is_some_and(|id| id != row.student_id)
            });
        if id_conflict || email_conflict {
            let reason = if id_conflict {
                "student_id_conflict"
            } else {
                "email_conflict"
            };
            invalid.push(StudentImportError {
                line: row.line,
                reason: reason.into(),
            });
            continue;
        }
        let user_id = if let Some(account) = accounts.first() {
            // 既有帳號的電子郵件只由學生自己填：Google 登入靠電子郵件比對帳號，
            // 匯入若能補上信箱，知道學號的人就能接管沒有信箱的學生帳號。
            if account.student_id.is_none() {
                sqlx::query(
                    "UPDATE users SET
                         student_id = COALESCE(student_id, $1),
                         imported_name = COALESCE(imported_name, $2),
                         display_name = CASE WHEN student_id IS NULL THEN $2 ELSE display_name END
                     WHERE id = $3",
                )
                .bind(&row.student_id)
                .bind(&row.name)
                .bind(account.id)
                .execute(&mut *tx)
                .await?;
            }
            account.id
        } else {
            let temporary_password = password::generate_temporary();
            let hash = hash_blocking(temporary_password.clone()).await?;
            let id = roster_store::create_student(
                &mut tx,
                &row.student_id,
                &row.name,
                row.email.as_deref(),
                &hash,
            )
            .await?;
            credentials.push(Credential {
                student_id: row.student_id.clone(),
                temporary_password,
            });
            id
        };
        if roster_store::add_user(&mut tx, classroom.id, user_id).await? {
            added += 1;
            audit(
                &mut *tx,
                cu.user.id,
                "roster_added",
                row.email.as_deref().unwrap_or(&user_id.to_string()),
                None,
            )
            .await?;
        } else {
            existing += 1;
        }
    }
    tx.commit().await?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(ImportResult {
            added,
            existing,
            errors: invalid.len(),
            invalid,
            credentials,
        }),
    ))
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
    let user_id = roster_store::remove_user(&mut tx, classroom.id, &email)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let target = if email.contains('@') {
        email.to_lowercase()
    } else {
        user_id.to_string()
    };
    audit(&mut *tx, cu.user.id, "roster_removed", &target, None).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
