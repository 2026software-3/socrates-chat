//! 教師管理自己的 AI 供應商 API key（F-40.1-BE，#592；S-03.1「多供應商與教師選模型」）。
//!
//! 只有教師本人能新增、更換、刪除與測試自己的 key；管理者不能，管理者的檢視與刪除另見 F-40.9。
//! 他人的 key 與不存在的 key 一律回相同的 404。回應、錯誤與日誌都不含 key 或密文，只回遮蔽識別。

use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post, put},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    AppState,
    ai_providers::{self, KeyCheck},
    auth::{CurrentUser, RequireTeacher},
    error::ApiError,
    teacher_key_crypto::KeyCipher,
    teacher_key_store::{self as store, AddError, TeacherKey},
};

/// 每位教師在一個時間窗內最多按幾次「測試 key」（會對供應商發出請求）。暫定值，待需求方確認。
pub const KEY_TEST_MAX: usize = 10;
pub const KEY_TEST_WINDOW: Duration = Duration::from_secs(600);
/// 每位教師每分鐘最多取幾次模型清單（取清單不產生費用，只擋重複點擊與腳本）。暫定值，待需求方確認。
pub const MODEL_LIST_MAX_CALLS: usize = 30;
pub const MODEL_LIST_WINDOW: Duration = Duration::from_secs(60);
/// 同一供應商有多把 key 時，最多依序試幾把。
const MODEL_LIST_MAX_KEYS: usize = 3;
const MAX_KEY_CHARS: usize = 512;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/teacher/ai-providers", get(list_providers))
        .route("/api/teacher/keys", get(list_keys).post(add_key))
        .route("/api/teacher/keys/models", get(list_models))
        .route(
            "/api/teacher/keys/{id}",
            put(replace_key).delete(delete_key),
        )
        .route("/api/teacher/keys/{id}/test", post(test_key))
}

/// 只有教師本人（不含只有管理者身分的人）。
fn teacher_only(cu: &CurrentUser) -> Result<(), ApiError> {
    if cu.roles.teacher {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "forbidden",
            "只有教師可管理自己的 API key",
        ))
    }
}

fn cipher(state: &AppState) -> Result<&KeyCipher, ApiError> {
    state
        .config
        .teacher_key_cipher
        .as_deref()
        .ok_or(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "teacher_keys_unavailable",
            "尚未設定 API key 的加密金鑰，暫時無法儲存 key",
        ))
}

fn parse_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(raw).map_err(|_| ApiError::not_found())
}

const INVALID_KEY: ApiError = ApiError::bad_request("invalid_key", "API key 格式不正確");
const DUPLICATE: ApiError = ApiError::conflict("key_already_exists", "這把 key 已經存在");

/// 去掉前後空白後，必須非空、不超過上限，且不含空白或控制字元。
fn clean_key(raw: &str) -> Result<&str, ApiError> {
    let key = raw.trim();
    if key.is_empty()
        || key.chars().count() > MAX_KEY_CHARS
        || key.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(INVALID_KEY);
    }
    Ok(key)
}

fn add_error(e: AddError) -> ApiError {
    match e {
        AddError::Duplicate => DUPLICATE,
        AddError::Db(e) => e.into(),
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ProviderItem {
    #[schema(example = "openai")]
    id: &'static str,
    #[schema(example = "OpenAI")]
    name: &'static str,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct KeyItem {
    id: Uuid,
    provider: String,
    /// 遮蔽識別，例如「…abcd」；完整 key 不會再顯示
    mask: String,
    /// `ok`、`invalid`（失效或遭撤銷）、`rate_limited`、`quota_exhausted`
    status: &'static str,
    /// 進入目前狀態的時間；`ok` 時為 null
    status_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<TeacherKey> for KeyItem {
    fn from(k: TeacherKey) -> Self {
        Self {
            id: k.id,
            provider: k.provider,
            mask: k.mask,
            status: k.status.as_str(),
            status_at: k.status_at,
            created_at: k.created_at,
            updated_at: k.updated_at,
        }
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewKey {
    #[schema(example = "openai")]
    provider: String,
    #[schema(max_length = 512)]
    key: String,
}

/// 更換 key：供應商不可變更，要換供應商請新增另一把。
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReplaceKey {
    #[schema(max_length = 512)]
    key: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct KeyTestResult {
    /// `usable` 或 `connection_failed`；額度不足仍算可用，key 無效或已撤銷算連線失敗
    result: &'static str,
}

/// 開放給教師的供應商名單。
#[utoipa::path(
    get,
    path = "/api/teacher/ai-providers",
    tag = "teacher-keys",
    summary = "供應商名單",
    responses((status = 200, description = "可綁定 key 的供應商", body = Vec<ProviderItem>), crate::error::CommonErrors)
)]
pub(crate) async fn list_providers(
    RequireTeacher(cu): RequireTeacher,
) -> Result<Json<Vec<ProviderItem>>, ApiError> {
    teacher_only(&cu)?;
    Ok(Json(
        ai_providers::providers()
            .iter()
            .map(|p| ProviderItem {
                id: p.id,
                name: p.name,
            })
            .collect(),
    ))
}

/// 列出自己的 key（遮蔽識別與狀態）。
#[utoipa::path(
    get,
    path = "/api/teacher/keys",
    tag = "teacher-keys",
    summary = "列出自己的 API key",
    responses((status = 200, description = "key 清單，不含完整 key", body = Vec<KeyItem>), crate::error::CommonErrors)
)]
pub(crate) async fn list_keys(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
) -> Result<Json<Vec<KeyItem>>, ApiError> {
    teacher_only(&cu)?;
    let keys = store::list_for_owner(&state.pool, cu.user.id).await?;
    Ok(Json(keys.into_iter().map(Into::into).collect()))
}

/// 新增一把 key；同一把 key（不論哪位教師）不能重複。
#[utoipa::path(
    post,
    path = "/api/teacher/keys",
    tag = "teacher-keys",
    summary = "新增 API key",
    request_body = NewKey,
    responses(
        (status = 201, description = "已新增", body = KeyItem),
        (status = 400, description = "invalid_provider、invalid_key", body = crate::error::ErrorBody),
        (status = 409, description = "key_already_exists", body = crate::error::ErrorBody),
        (status = 503, description = "teacher_keys_unavailable", body = crate::error::ErrorBody),
        crate::error::CommonErrors
    )
)]
pub(crate) async fn add_key(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Json(body): Json<NewKey>,
) -> Result<(StatusCode, Json<KeyItem>), ApiError> {
    teacher_only(&cu)?;
    let provider = ai_providers::find(&body.provider).ok_or(ApiError::bad_request(
        "invalid_provider",
        "不支援這個供應商",
    ))?;
    let key = clean_key(&body.key)?;
    let created = store::add(&state.pool, cipher(&state)?, cu.user.id, provider.id, key)
        .await
        .map_err(add_error)?;
    Ok((StatusCode::CREATED, Json(created.into())))
}

/// 更換自己的一把 key：id、供應商與歷史用量不變。
#[utoipa::path(
    put,
    path = "/api/teacher/keys/{id}",
    tag = "teacher-keys",
    summary = "更換 API key",
    params(("id" = Uuid, Path, description = "key ID")),
    request_body = ReplaceKey,
    responses(
        (status = 200, description = "已更換", body = KeyItem),
        (status = 400, description = "invalid_key", body = crate::error::ErrorBody),
        (status = 409, description = "key_already_exists", body = crate::error::ErrorBody),
        (status = 503, description = "teacher_keys_unavailable", body = crate::error::ErrorBody),
        crate::error::CommonErrors
    )
)]
pub(crate) async fn replace_key(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
    Json(body): Json<ReplaceKey>,
) -> Result<Json<KeyItem>, ApiError> {
    teacher_only(&cu)?;
    let id = parse_id(&raw_id)?;
    // 先確認是自己的 key，他人的 key 不因為輸入錯誤而得到不同的回應
    if !store::owns(&state.pool, cu.user.id, id).await? {
        return Err(ApiError::not_found());
    }
    let key = clean_key(&body.key)?;
    store::replace(&state.pool, cipher(&state)?, cu.user.id, id, key)
        .await
        .map_err(add_error)?
        .map(|k| Json(k.into()))
        .ok_or_else(ApiError::not_found)
}

/// 刪除自己的一把 key；該 key 的用量統計連帶刪除，教室統計隨之扣除。
#[utoipa::path(
    delete,
    path = "/api/teacher/keys/{id}",
    tag = "teacher-keys",
    summary = "刪除 API key",
    params(("id" = Uuid, Path, description = "key ID")),
    responses((status = 204, description = "已刪除"), crate::error::CommonErrors)
)]
pub(crate) async fn delete_key(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    teacher_only(&cu)?;
    if store::delete(&state.pool, cu.user.id, parse_id(&raw_id)?).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found())
    }
}

/// 測試自己的一把 key 是否可用：只呼叫供應商的模型清單端點，不送對話內容；可能產生 API 請求與費用。
#[utoipa::path(
    post,
    path = "/api/teacher/keys/{id}/test",
    tag = "teacher-keys",
    summary = "測試 API key",
    params(("id" = Uuid, Path, description = "key ID")),
    responses(
        (status = 200, description = "測試結果", body = KeyTestResult),
        (status = 429, description = "key_test_rate_limited", body = crate::error::ErrorBody),
        (status = 503, description = "teacher_keys_unavailable", body = crate::error::ErrorBody),
        crate::error::CommonErrors
    )
)]
pub(crate) async fn test_key(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
) -> Result<Json<KeyTestResult>, ApiError> {
    teacher_only(&cu)?;
    let id = parse_id(&raw_id)?;
    let cipher = cipher(&state)?;
    let Some((provider_id, key)) = store::get_secret(&state.pool, cipher, cu.user.id, id).await?
    else {
        // 清單裡看得到、但解不開（加密金鑰已輪替掉）的 key 一律算連線失敗，不當成不存在
        return if store::owns(&state.pool, cu.user.id, id).await? {
            Ok(Json(KeyTestResult {
                result: "connection_failed",
            }))
        } else {
            Err(ApiError::not_found())
        };
    };
    let provider = ai_providers::find(&provider_id).ok_or_else(ApiError::not_found)?;
    if state.key_test_limit.try_acquire(cu.user.id).is_none() {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "key_test_rate_limited",
            "測試太頻繁，請稍後再試",
        ));
    }
    let result = match state.key_verifier.verify_key(provider, key.secret()).await {
        KeyCheck::Usable => {
            // 測試可用就清掉先前記下的失效／額度／限流狀態，路由不必等冷卻結束；
            // 連線失敗可能只是網路問題，不改寫狀態
            store::set_status_for(&state.pool, &key, store::KeyStatus::Ok).await?;
            "usable"
        }
        KeyCheck::ConnectionFailed => "connection_failed",
    };
    Ok(Json(KeyTestResult { result }))
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ModelsQuery {
    /// 供應商識別碼，例如 `openai`
    provider: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ModelList {
    provider: String,
    /// `ok`：`models` 是供應商目前列出的模型；`no_key`：這位教師沒有該供應商的 key；
    /// `unavailable`：有 key 但取不到清單（key 無效、供應商不提供清單、逾時等）。後兩者前端退回手動輸入。
    status: &'static str,
    /// 只是輸入建議，不是白名單：儲存時仍以模型驗證為準。已排序、去重，最多 500 個。
    models: Vec<String>,
}

/// 用自己的 key 向供應商取模型清單，給教室 AI 設定的模型欄位當輸入建議。
/// 清單只是建議，教師仍可輸入清單以外的模型 ID；取不到時回 200 與 `status`，不是錯誤。
/// 不改動 key 的狀態，回應與日誌都不含 key。
#[utoipa::path(
    get,
    path = "/api/teacher/keys/models",
    tag = "teacher-keys",
    summary = "供應商的模型清單（輸入建議）",
    params(ModelsQuery),
    responses(
        (status = 200, description = "模型清單，或沒有 key／取不到的狀態", body = ModelList),
        (status = 400, description = "不支援的供應商（invalid_provider）"),
        (status = 429, description = "太頻繁（model_list_rate_limited）"),
        crate::error::CommonErrors
    )
)]
pub(crate) async fn list_models(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Query(query): Query<ModelsQuery>,
) -> Result<Json<ModelList>, ApiError> {
    teacher_only(&cu)?;
    let provider = ai_providers::find(&query.provider).ok_or(ApiError::bad_request(
        "invalid_provider",
        "不支援這個供應商",
    ))?;
    let cipher = cipher(&state)?;
    if state.model_list_limit.try_acquire(cu.user.id).is_none() {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "model_list_rate_limited",
            "操作太頻繁，請稍後再試",
        ));
    }
    let keys = store::keys_for(&state.pool, cipher, cu.user.id, provider.id).await?;
    let respond = |status, models| {
        Json(ModelList {
            provider: provider.id.to_owned(),
            status,
            models,
        })
    };
    if keys.is_empty() {
        return Ok(respond("no_key", Vec::new()));
    }
    for key in keys.iter().take(MODEL_LIST_MAX_KEYS) {
        if let Ok(models) = state.model_lister.list_models(provider, key.secret()).await {
            return Ok(respond("ok", models));
        }
    }
    Ok(respond("unavailable", Vec::new()))
}
