//! 教師查看自己 key 與教室的 AI 用量（F-40.8-BE，#603；S-03.1）。
//!
//! key 的用量只有持有的教師本人看得到，系統管理者不能查（規格）；教室用量教師看自己建立的教室，管理者可看任何教室（需求方已確認）。
//! 只回次數與 token，不計金額。

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    AppState,
    auth::RequireTeacher,
    classroom_store,
    error::ApiError,
    teacher_key_store as keys,
    teacher_key_usage::{self as usage, ModelUsage, Range, Totals},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/teacher/keys/{id}/usage", get(key_usage))
        .route("/api/classrooms/{id}/ai-usage", get(classroom_usage))
}

#[derive(Deserialize)]
pub(crate) struct RangeQuery {
    pub(crate) range: Option<String>,
}

/// 沒帶 `range` 時預設最近 7 天（暫定，待需求方確認）。
pub(crate) fn parse_range(q: &RangeQuery) -> Result<(Range, &'static str), ApiError> {
    match q.range.as_deref().unwrap_or("7d") {
        "today" => Ok((Range::Today, "today")),
        "7d" => Ok((Range::Last7Days, "7d")),
        "30d" => Ok((Range::Last30Days, "30d")),
        "all" => Ok((Range::All, "all")),
        _ => Err(ApiError::bad_request(
            "invalid_range",
            "range 必須是 today、7d、30d 或 all",
        )),
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct UsageNumbers {
    requests: i64,
    input_tokens: i64,
    output_tokens: i64,
    /// 供應商沒有回報 token 的請求數；這些請求不估算 token
    unreported_requests: i64,
}

impl From<Totals> for UsageNumbers {
    fn from(t: Totals) -> Self {
        Self {
            requests: t.requests,
            input_tokens: t.input_tokens,
            output_tokens: t.output_tokens,
            unreported_requests: t.unreported_requests,
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ModelUsageItem {
    provider: String,
    model: String,
    #[serde(flatten)]
    numbers: UsageNumbers,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct UsageReport {
    /// `today`、`7d`、`30d` 或 `all`
    range: &'static str,
    totals: UsageNumbers,
    /// 依（供應商、模型）細分；備援模型的用量歸在它自己的（供應商、模型）
    items: Vec<ModelUsageItem>,
}

pub(crate) fn report(range: &'static str, rows: Vec<ModelUsage>) -> UsageReport {
    let mut sum = Totals::default();
    for r in &rows {
        sum.requests += r.totals.requests;
        sum.input_tokens += r.totals.input_tokens;
        sum.output_tokens += r.totals.output_tokens;
        sum.unreported_requests += r.totals.unreported_requests;
    }
    UsageReport {
        range,
        totals: sum.into(),
        items: rows
            .into_iter()
            .map(|r| ModelUsageItem {
                provider: r.provider,
                model: r.model,
                numbers: r.totals.into(),
            })
            .collect(),
    }
}

fn parse_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(raw).map_err(|_| ApiError::not_found())
}

/// 自己某把 key 的用量。
#[utoipa::path(
    get,
    path = "/api/teacher/keys/{id}/usage",
    tag = "teacher-keys",
    summary = "查看 key 的用量",
    params(("id" = Uuid, Path, description = "key ID"), ("range" = Option<String>, Query, description = "today、7d（預設）、30d、all")),
    responses((status = 200, description = "用量", body = UsageReport), (status = 400, description = "invalid_range", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn key_usage(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
    Query(q): Query<RangeQuery>,
) -> Result<Json<UsageReport>, ApiError> {
    if !cu.roles.teacher {
        return Err(ApiError::forbidden(
            "forbidden",
            "只有教師可查看自己 key 的用量",
        ));
    }
    let id = parse_id(&raw_id)?;
    if !keys::owns(&state.pool, cu.user.id, id).await? {
        return Err(ApiError::not_found());
    }
    let (range, name) = parse_range(&q)?;
    Ok(Json(report(
        name,
        usage::by_model_for_key(&state.pool, id, range).await?,
    )))
}

/// 教室的用量（各 key 加總）：教師看自己建立的教室，管理者看任何教室。
#[utoipa::path(
    get,
    path = "/api/classrooms/{id}/ai-usage",
    tag = "classrooms",
    summary = "查看教室的 AI 用量",
    params(("id" = Uuid, Path, description = "教室 ID"), ("range" = Option<String>, Query, description = "today、7d（預設）、30d、all")),
    responses((status = 200, description = "用量", body = UsageReport), (status = 400, description = "invalid_range", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn classroom_usage(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
    Query(q): Query<RangeQuery>,
) -> Result<Json<UsageReport>, ApiError> {
    let id = parse_id(&raw_id)?;
    // 教師只能看自己建立的教室（他人的教室與不存在相同 404）；管理者可以看任何教室的用量
    classroom_store::get_visible(&state.pool, id, cu.user.id, cu.roles.admin)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let (range, name) = parse_range(&q)?;
    Ok(Json(report(
        name,
        usage::by_model_for_classroom(&state.pool, id, range).await?,
    )))
}
