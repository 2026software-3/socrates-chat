//! 教室的 AI 對話模型與備援設定（F-40.4-BE，#597；S-03.1）。
//!
//! 只有教室建立者（教師）能讀寫；管理者能否檢視規格未明，預設不開放（待確認）。
//! 儲存前先確認教師有每個供應商的 key，再用 F-40.3 的 `verify_model` 驗證每個模型，全部通過才儲存。
//! 學生只看得到供應商名稱，看不到模型 ID、備援或 key。

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    AppState,
    ai_providers::{self, ModelCheckError, Provider},
    auth::{RequireStudent, RequireTeacher},
    classroom_ai_store::{self as store, MAX_FALLBACKS, ModelRef, SetError},
    classroom_store,
    error::ApiError,
    teacher_key_store as keys,
};

const MAX_MODEL_CHARS: usize = 200;
const FALLBACK_TARGETS: [&str; 3] = ["fallback_1", "fallback_2", "fallback_3"];

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/classrooms/{id}/ai-settings",
            get(get_settings).put(put_settings),
        )
        .route("/api/classrooms/{id}/ai-provider", get(get_provider))
}

fn parse_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(raw).map_err(|_| ApiError::not_found())
}

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub(crate) struct ModelChoice {
    #[schema(example = "openai")]
    provider: String,
    /// 教師自由輸入的模型名稱
    #[schema(max_length = 200)]
    model: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct AiSettings {
    /// 過渡中：NTNU 上線前已存在的教室，沿用系統 key，直到教師設定第一把 key 與教室模型
    transitional: bool,
    primary: Option<ModelChoice>,
    /// 依嘗試順序，最多 3 個
    fallbacks: Vec<ModelChoice>,
    /// 已確認過資料流的供應商
    confirmed_providers: Vec<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct PutSettings {
    provider: String,
    #[schema(max_length = 200)]
    model: String,
    #[serde(default)]
    fallbacks: Vec<ModelChoice>,
    /// 第一次為這間教室選定某家供應商時必須為 true：教師已確認該教室的對話內容會送往該供應商
    #[serde(default)]
    data_flow_confirmed: bool,
}

fn choice(m: ModelRef) -> ModelChoice {
    ModelChoice {
        provider: m.provider,
        model: m.model,
    }
}

async fn view(state: &AppState, classroom: Uuid) -> Result<AiSettings, ApiError> {
    let setting = store::get(&state.pool, classroom).await?;
    Ok(AiSettings {
        transitional: store::is_transitional(&state.pool, classroom).await?,
        primary: setting.as_ref().map(|s| choice(s.primary.clone())),
        fallbacks: setting
            .map(|s| s.fallbacks.into_iter().map(choice).collect())
            .unwrap_or_default(),
        confirmed_providers: store::confirmed_providers(&state.pool, classroom).await?,
    })
}

/// 只有建立者：管理者身分不放行；他人的教室與不存在相同 404。
async fn owned_classroom(
    state: &AppState,
    cu: &crate::auth::CurrentUser,
    raw_id: &str,
) -> Result<classroom_store::Classroom, ApiError> {
    if !cu.roles.teacher {
        return Err(ApiError::forbidden(
            "forbidden",
            "只有教室的教師可管理 AI 設定",
        ));
    }
    classroom_store::get_visible(&state.pool, parse_id(raw_id)?, cu.user.id, false)
        .await?
        .ok_or_else(ApiError::not_found)
}

#[utoipa::path(
    get,
    path = "/api/classrooms/{id}/ai-settings",
    tag = "classrooms",
    summary = "查看教室的 AI 模型設定",
    params(("id" = Uuid, Path, description = "教室 ID")),
    responses((status = 200, description = "設定", body = AiSettings), crate::error::CommonErrors)
)]
pub(crate) async fn get_settings(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
) -> Result<Json<AiSettings>, ApiError> {
    let classroom = owned_classroom(&state, &cu, &raw_id).await?;
    Ok(Json(view(&state, classroom.id).await?))
}

pub(crate) fn check_error(reason: ModelCheckError, target: &'static str) -> ApiError {
    let (code, message) = match reason {
        ModelCheckError::AuthFailed => ("model_auth_failed", "供應商拒絕了這把 key"),
        ModelCheckError::ModelNotFound => ("model_not_found", "找不到這個模型"),
        ModelCheckError::StreamingUnsupported => {
            ("model_streaming_unsupported", "這個模型不支援串流")
        }
        ModelCheckError::StructuredOutputUnsupported => (
            "model_structured_output_unsupported",
            "這個模型不支援結構化輸出",
        ),
        ModelCheckError::Other => ("model_check_failed", "模型驗證失敗"),
    };
    ApiError::bad_request(code, message).with_target(target)
}

struct Entry {
    target: &'static str,
    provider: &'static Provider,
    model: String,
}

fn entry(target: &'static str, provider: &str, model: &str) -> Result<Entry, ApiError> {
    let provider = ai_providers::find(provider)
        .ok_or(ApiError::bad_request("invalid_provider", "不支援這個供應商").with_target(target))?;
    let model = model.trim();
    if model.is_empty() || model.chars().count() > MAX_MODEL_CHARS {
        return Err(
            ApiError::bad_request("invalid_model", "模型名稱不可為空或超過 200 個字元")
                .with_target(target),
        );
    }
    Ok(Entry {
        target,
        provider,
        model: model.to_owned(),
    })
}

/// 儲存教室的主模型與備援。會對供應商送出少量請求驗證每個模型（可能產生少量費用），
/// 與「測試 key」共用每人頻率限制。
#[utoipa::path(
    put,
    path = "/api/classrooms/{id}/ai-settings",
    tag = "classrooms",
    summary = "設定教室的 AI 模型與備援",
    params(("id" = Uuid, Path, description = "教室 ID")),
    request_body = PutSettings,
    responses(
        (status = 200, description = "已儲存", body = AiSettings),
        (status = 400, description = "invalid_provider、invalid_model、too_many_fallbacks、no_key_for_provider、data_flow_confirmation_required、model_auth_failed、model_not_found、model_streaming_unsupported、model_structured_output_unsupported、model_check_failed；錯誤可帶 target（primary、fallback_1…3）", body = crate::error::ErrorBody),
        (status = 409, description = "classroom_archived", body = crate::error::ErrorBody),
        (status = 429, description = "key_test_rate_limited", body = crate::error::ErrorBody),
        (status = 503, description = "teacher_keys_unavailable", body = crate::error::ErrorBody),
        crate::error::CommonErrors
    )
)]
pub(crate) async fn put_settings(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
    Json(body): Json<PutSettings>,
) -> Result<Json<AiSettings>, ApiError> {
    let classroom = owned_classroom(&state, &cu, &raw_id).await?;
    if classroom.is_archived {
        return Err(ApiError::classroom_archived());
    }
    if body.fallbacks.len() > MAX_FALLBACKS {
        return Err(ApiError::bad_request(
            "too_many_fallbacks",
            "備援模型最多三個",
        ));
    }
    let mut entries = vec![entry("primary", &body.provider, &body.model)?];
    for (i, f) in body.fallbacks.iter().enumerate() {
        entries.push(entry(FALLBACK_TARGETS[i], &f.provider, &f.model)?);
    }

    // 每個供應商都要有教師自己的 key（可跨供應商）
    let cipher = state
        .config
        .teacher_key_cipher
        .as_deref()
        .ok_or(ApiError::new(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "teacher_keys_unavailable",
            "尚未設定 API key 的加密金鑰，暫時無法儲存 key",
        ))?;
    let mut secrets = Vec::new();
    for e in &entries {
        let mut usable = keys::keys_for(&state.pool, cipher, cu.user.id, e.provider.id).await?;
        // 優先用狀態正常的 key
        usable.sort_by_key(|k| k.status != keys::KeyStatus::Ok);
        let key = usable.into_iter().next().ok_or(
            ApiError::bad_request("no_key_for_provider", "請先綁定這個供應商的 API key")
                .with_target(e.target),
        )?;
        secrets.push(key);
    }

    // 第一次選定某家供應商（含備援）要先確認資料流
    let confirmed = store::confirmed_providers(&state.pool, classroom.id).await?;
    if !body.data_flow_confirmed
        && entries
            .iter()
            .any(|e| !confirmed.iter().any(|c| c == e.provider.id))
    {
        return Err(ApiError::bad_request(
            "data_flow_confirmation_required",
            "第一次選定供應商前，必須確認教室對話會送往該供應商",
        ));
    }

    if state.key_test_limit.try_acquire(cu.user.id).is_none() {
        return Err(ApiError::new(
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            "key_test_rate_limited",
            "驗證太頻繁，請稍後再試",
        ));
    }
    // 各模型同時驗證（最多四個，各自有逾時）；失敗時回報清單中第一個不通過的
    let checks = futures_util::future::join_all(entries.iter().zip(&secrets).map(|(e, key)| {
        state
            .model_verifier
            .verify_model(e.provider, &e.model, key.secret())
    }))
    .await;
    for (e, check) in entries.iter().zip(checks) {
        check.map_err(|reason| check_error(reason, e.target))?;
    }

    let mut it = entries.iter();
    let primary = it.next().expect("primary entry");
    let fallbacks: Vec<ModelRef> = it
        .map(|e| ModelRef {
            provider: e.provider.id.to_owned(),
            model: e.model.clone(),
        })
        .collect();
    store::set(
        &state.pool,
        classroom.id,
        ModelRef {
            provider: primary.provider.id.to_owned(),
            model: primary.model.clone(),
        },
        fallbacks,
    )
    .await
    .map_err(|e| match e {
        SetError::TooManyFallbacks => {
            ApiError::bad_request("too_many_fallbacks", "備援模型最多三個")
        }
        SetError::Invalid => ApiError::bad_request("invalid_model", "模型設定格式不正確"),
        SetError::Db(e) => e.into(),
    })?;
    for e in &entries {
        store::confirm(&state.pool, classroom.id, e.provider.id, cu.user.id).await?;
    }
    // 教師已有 key 且儲存了有效設定：之後不再退回系統 key
    store::clear_transitional(&state.pool, classroom.id).await?;
    Ok(Json(view(&state, classroom.id).await?))
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ProviderName {
    id: &'static str,
    name: &'static str,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ClassroomProvider {
    /// 本教室的 AI 供應商；尚未設定或過渡中（沿用系統 key）時為 null
    provider: Option<ProviderName>,
}

/// 教室成員看到的供應商名稱（「本教室的 AI 由 {供應商} 提供」）；看不到模型、備援或 key。
#[utoipa::path(
    get,
    path = "/api/classrooms/{id}/ai-provider",
    tag = "classrooms",
    summary = "查看教室的 AI 供應商（學生）",
    params(("id" = Uuid, Path, description = "教室 ID")),
    responses((status = 200, description = "供應商名稱", body = ClassroomProvider), crate::error::CommonErrors)
)]
pub(crate) async fn get_provider(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
) -> Result<Json<ClassroomProvider>, ApiError> {
    let id = parse_id(&raw_id)?;
    let member: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM enrollments WHERE classroom_id = $1 AND user_id = $2)",
    )
    .bind(id)
    .bind(cu.user.id)
    .fetch_one(&state.pool)
    .await?;
    if !member {
        return Err(ApiError::not_found());
    }
    let provider = if store::is_transitional(&state.pool, id).await? {
        None
    } else {
        store::get(&state.pool, id)
            .await?
            .and_then(|s| ai_providers::find(&s.primary.provider))
            .map(|p| ProviderName {
                id: p.id,
                name: p.name,
            })
    };
    Ok(Json(ClassroomProvider { provider }))
}
