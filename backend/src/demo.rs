//! 示範模式：發表用的固定帳號。密碼是公開的，所以只在 `DEMO_MODE=true` 時建立與列出，
//! 且不含管理者；既有的管理者帳號即使信箱相同也不會被改動。

use axum::{Json, Router, extract::State, routing::get};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::{AppState, error::ApiError, password};

const ACCOUNTS: &str = include_str!("demo_accounts.json");

#[derive(Deserialize)]
struct Roster {
    teacher: Teacher,
    student_password: String,
    students: Vec<Student>,
}

#[derive(Deserialize)]
struct Teacher {
    email: String,
    name: String,
    password: String,
}

#[derive(Deserialize)]
struct Student {
    student_id: String,
    name: String,
    language: String,
}

fn roster() -> Roster {
    serde_json::from_str(ACCOUNTS).expect("demo_accounts.json is valid")
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/demo/accounts", get(accounts))
}

/// 建立或重設示範帳號：密碼回到公開的固定值、解除停用、不要求改密碼。
/// 已存在的管理者帳號不動。
pub async fn seed_accounts(pool: &PgPool) -> Result<(), sqlx::Error> {
    let r = roster();
    let teacher_hash = password::hash(&r.teacher.password).expect("hash demo password");
    let student_hash = password::hash(&r.student_password).expect("hash demo password");
    sqlx::query("INSERT INTO teachers (email) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(&r.teacher.email)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO users (email, password_hash, must_change_password, display_name, language, language_initialized)
         VALUES ($1, $2, false, $3, 'zh-TW', true)
         ON CONFLICT (email) DO UPDATE
           SET password_hash = $2, must_change_password = false, disabled_at = NULL, updated_at = now()
           WHERE users.is_admin = false",
    )
    .bind(&r.teacher.email)
    .bind(&teacher_hash)
    .bind(&r.teacher.name)
    .execute(pool)
    .await?;
    for s in &r.students {
        sqlx::query(
            "INSERT INTO users (student_id, imported_name, password_hash, must_change_password, display_name, language, language_initialized)
             VALUES ($1, $2, $3, false, $2, $4, true)
             ON CONFLICT (student_id) DO UPDATE
               SET password_hash = $3, must_change_password = false, disabled_at = NULL, updated_at = now()",
        )
        .bind(&s.student_id)
        .bind(&s.name)
        .bind(&student_hash)
        .bind(&s.language)
        .execute(pool)
        .await?;
    }
    Ok(())
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct DemoAccount {
    /// `teacher` 或 `student`
    role: &'static str,
    /// 顯示名稱
    label: String,
    /// 登入欄位要填的值：教師是電子郵件，學生是學號
    account: String,
    password: String,
    /// 介面語言，讓發表者看得出哪位是外語學生
    language: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct DemoAccounts {
    accounts: Vec<DemoAccount>,
}

/// 示範帳號清單（含公開的固定密碼）。沒開示範模式時回 404，與不存在的路徑無從區分。
#[utoipa::path(
    get,
    path = "/api/demo/accounts",
    tag = "demo",
    summary = "示範帳號（僅 DEMO_MODE）",
    security(()),
    responses(
        (status = 200, description = "示範帳號", body = DemoAccounts),
        (status = 404, description = "未啟用示範模式", body = crate::error::ErrorBody)
    )
)]
pub(crate) async fn accounts(
    State(state): State<AppState>,
) -> Result<Json<DemoAccounts>, ApiError> {
    if !state.config.demo_mode {
        return Err(ApiError::not_found());
    }
    let r = roster();
    let mut accounts = vec![DemoAccount {
        role: "teacher",
        label: r.teacher.name,
        account: r.teacher.email,
        password: r.teacher.password,
        language: "zh-TW".into(),
    }];
    accounts.extend(r.students.into_iter().map(|s| DemoAccount {
        role: "student",
        label: s.name,
        account: s.student_id,
        password: r.student_password.clone(),
        language: s.language,
    }));
    Ok(Json(DemoAccounts { accounts }))
}
