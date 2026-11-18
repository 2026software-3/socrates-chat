//! 管理者在介面管理系統 AI：供應商、模型與系統 key（多把輪流使用）。
//!
//! 系統用途 = 沒有教室的個人練習、過渡中的教室、翻譯、主張分組、維度與公告翻譯等不屬於任何教師的 AI 呼叫。
//! key 以與教師 key 相同的方式加密存資料庫（`TEACHER_KEY_ENCRYPTION_KEYS`）；回應、錯誤與日誌都不含 key 或密文，
//! 只回遮蔽識別。沒有設定時照舊使用主機上的 `.apikeylist`（`AppState::system_route`）。
//! 稽核只記操作者與供應商識別碼，不記 key、遮蔽識別或模型內容。語音（STT／TTS）不在這裡。

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{get, post, put},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    AppState, accounts,
    ai_providers::{self, KeyCheck},
    auth::RequireAdmin,
    classroom_ai_settings::check_error,
    error::ApiError,
    system_key_store::{self as store, SystemKey},
    teacher_key_store::KeyStatus,
    teacher_keys::{self, KeyTestResult, NewKey, ReplaceKey},
};

const MAX_MODEL_CHARS: usize = 200;
const USAGE_DAYS: i32 = 30;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/system-ai", get(overview))
        .route(
            "/api/admin/system-ai/model",
            put(set_model).delete(clear_model),
        )
        .route("/api/admin/system-ai/keys", post(add_key))
        .route(
            "/api/admin/system-ai/keys/{id}",
            put(replace_key).delete(delete_key),
        )
        .route("/api/admin/system-ai/keys/{id}/test", post(test_key))
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ModelSetting {
    provider: String,
    provider_name: String,
    model: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct SystemKeyItem {
    id: Uuid,
    provider: String,
    provider_name: String,
    /// 遮蔽識別，例如「…abcd」；完整 key 不會再顯示
    mask: String,
    /// `ok`、`invalid`、`rate_limited`、`quota_exhausted`
    status: &'static str,
    status_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    /// 最近 30 天的請求數與 token（供應商沒回報的請求只計在 `unreported_requests_30d`）
    requests_30d: i64,
    input_tokens_30d: i64,
    output_tokens_30d: i64,
    unreported_requests_30d: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct Overview {
    /// `database`：系統用途用這裡設定的模型與 key；`host_file`：用主機上的 `.apikeylist`
    source: &'static str,
    /// 後端有沒有開教師 key 與路由功能（`CLASSROOM_AI_ROUTING`）；沒開時這裡的設定不會生效
    routing_enabled: bool,
    /// 有沒有設定 key 的加密金鑰（`TEACHER_KEY_ENCRYPTION_KEYS`）；沒有時無法新增 key
    encryption_configured: bool,
    setting: Option<ModelSetting>,
    /// 主機設定的模型（`OPENAI_MODEL`）：`source` 是 `host_file` 時使用它
    host_model: String,
    keys: Vec<SystemKeyItem>,
    /// 可選的供應商（管理者不能呼叫教師的供應商名單）
    providers: Vec<teacher_keys::ProviderItem>,
}

fn provider_name(id: &str) -> String {
    ai_providers::find(id).map_or_else(|| id.to_owned(), |p| p.name.to_owned())
}

async fn overview_of(state: &AppState) -> Result<Overview, ApiError> {
    let keys = store::list(&state.pool).await?;
    let usage = store::usage_by_key(&state.pool, USAGE_DAYS).await?;
    let setting = store::get_setting(&state.pool).await?;
    let in_use = state.config.classroom_ai_routing
        && state.config.teacher_key_cipher.is_some()
        && setting
            .as_ref()
            .is_some_and(|s| keys.iter().any(|k| k.provider == s.provider));
    Ok(Overview {
        source: if in_use { "database" } else { "host_file" },
        routing_enabled: state.config.classroom_ai_routing,
        encryption_configured: state.config.teacher_key_cipher.is_some(),
        setting: setting.map(|s| ModelSetting {
            provider_name: provider_name(&s.provider),
            provider: s.provider,
            model: s.model,
        }),
        host_model: state.config.openai_model.clone(),
        keys: keys.into_iter().map(|k| item(k, &usage)).collect(),
        providers: ai_providers::providers()
            .iter()
            .map(|p| teacher_keys::ProviderItem {
                id: p.id,
                name: p.name,
            })
            .collect(),
    })
}

fn item(k: SystemKey, usage: &[(Uuid, i64, i64, i64, i64)]) -> SystemKeyItem {
    let (requests, input, output, unreported) = usage
        .iter()
        .find(|u| u.0 == k.id)
        .map_or((0, 0, 0, 0), |u| (u.1, u.2, u.3, u.4));
    SystemKeyItem {
        provider_name: provider_name(&k.provider),
        id: k.id,
        provider: k.provider,
        mask: k.mask,
        status: k.status.0.as_str(),
        status_at: k.status_at,
        created_at: k.created_at,
        requests_30d: requests,
        input_tokens_30d: input,
        output_tokens_30d: output,
        unreported_requests_30d: unreported,
    }
}

#[utoipa::path(
    get,
    path = "/api/admin/system-ai",
    tag = "admin",
    summary = "系統 AI 設定：模型、key 狀態與近 30 天用量",
    responses((status = 200, body = Overview), crate::error::CommonErrors)
)]
pub(crate) async fn overview(
    RequireAdmin(_): RequireAdmin,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(overview_of(&state).await?),
    ))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelBody {
    #[schema(example = "openai")]
    provider: String,
    #[schema(example = "gpt-4o-mini", max_length = 200)]
    model: String,
}

/// 設定系統用途的供應商與模型。儲存前用該供應商的一把系統 key 送一次最小的串流與 JSON 請求驗證
/// （會產生少量費用），通過才儲存；沒有該供應商的 key 時不能設定。
#[utoipa::path(
    put,
    path = "/api/admin/system-ai/model",
    tag = "admin",
    summary = "設定系統用途的供應商與模型（儲存前驗證）",
    request_body = ModelBody,
    responses(
        (status = 200, body = Overview),
        (status = 400, description = "供應商不支援、模型格式不正確、沒有該供應商的 key 或模型驗證失敗"),
        (status = 429, description = "驗證太頻繁（key_test_rate_limited）"),
        crate::error::CommonErrors
    )
)]
pub(crate) async fn set_model(
    RequireAdmin(cu): RequireAdmin,
    State(state): State<AppState>,
    Json(body): Json<ModelBody>,
) -> Result<Json<Overview>, ApiError> {
    let provider = ai_providers::find(&body.provider).ok_or(ApiError::bad_request(
        "invalid_provider",
        "不支援這個供應商",
    ))?;
    let model = body.model.trim();
    if model.is_empty() || model.chars().count() > MAX_MODEL_CHARS {
        return Err(ApiError::bad_request(
            "invalid_model",
            "模型名稱不可為空或超過 200 個字元",
        ));
    }
    let cipher = teacher_keys::cipher(&state)?;
    let keys = store::keys_for(&state.pool, cipher, provider.id).await?;
    let key = keys.first().ok_or(ApiError::bad_request(
        "no_key_for_provider",
        "還沒有這個供應商的系統 key，請先新增",
    ))?;
    if state.key_test_limit.try_acquire(cu.user.id).is_none() {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "key_test_rate_limited",
            "驗證太頻繁，請稍後再試",
        ));
    }
    state
        .model_verifier
        .verify_model(provider, model, key.secret())
        .await
        .map_err(|reason| check_error(reason, "primary"))?;
    let mut tx = state.pool.begin().await?;
    store::set_setting(&mut *tx, cu.user.id, provider.id, model).await?;
    accounts::audit(
        &mut *tx,
        cu.user.id,
        "system_ai_model_set",
        provider.id,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(overview_of(&state).await?))
}

/// 清除設定：系統用途回到主機上的 `.apikeylist`。key 保留。
#[utoipa::path(
    delete,
    path = "/api/admin/system-ai/model",
    tag = "admin",
    summary = "清除系統用途的模型設定（回到主機的 .apikeylist）",
    responses((status = 200, body = Overview), crate::error::CommonErrors)
)]
pub(crate) async fn clear_model(
    RequireAdmin(cu): RequireAdmin,
    State(state): State<AppState>,
) -> Result<Json<Overview>, ApiError> {
    let mut tx = state.pool.begin().await?;
    let previous = store::get_setting(&state.pool).await?;
    if store::clear_setting(&mut *tx).await? {
        let target = previous.map_or("system", |s| {
            ai_providers::find(&s.provider).map_or("system", |p| p.id)
        });
        accounts::audit(
            &mut *tx,
            cu.user.id,
            "system_ai_model_cleared",
            target,
            None,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(overview_of(&state).await?))
}

#[utoipa::path(
    post,
    path = "/api/admin/system-ai/keys",
    tag = "admin",
    summary = "新增系統 key",
    request_body = NewKey,
    responses(
        (status = 201, body = SystemKeyItem),
        (status = 400, description = "供應商不在名單（invalid_provider）或 key 格式不正確（invalid_key）"),
        (status = 409, description = "這把 key 已被系統或某位教師綁定（key_already_exists）"),
        crate::error::CommonErrors
    )
)]
pub(crate) async fn add_key(
    RequireAdmin(cu): RequireAdmin,
    State(state): State<AppState>,
    Json(body): Json<NewKey>,
) -> Result<(StatusCode, Json<SystemKeyItem>), ApiError> {
    let provider = ai_providers::find(&body.provider).ok_or(ApiError::bad_request(
        "invalid_provider",
        "不支援這個供應商",
    ))?;
    let key = teacher_keys::clean_key(&body.key)?;
    let cipher = teacher_keys::cipher(&state)?;
    let added = store::add(&state.pool, cipher, cu.user.id, provider.id, key)
        .await
        .map_err(teacher_keys::add_error)?;
    accounts::audit(
        &state.pool,
        cu.user.id,
        "system_key_added",
        provider.id,
        None,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(item(added, &[]))))
}

#[utoipa::path(
    put,
    path = "/api/admin/system-ai/keys/{id}",
    tag = "admin",
    summary = "更換系統 key（供應商與歷史用量不變）",
    params(("id" = Uuid, Path)),
    request_body = ReplaceKey,
    responses((status = 200, body = SystemKeyItem), crate::error::CommonErrors)
)]
pub(crate) async fn replace_key(
    RequireAdmin(cu): RequireAdmin,
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
    Json(body): Json<ReplaceKey>,
) -> Result<Json<SystemKeyItem>, ApiError> {
    let id = teacher_keys::parse_id(&raw_id)?;
    let key = teacher_keys::clean_key(&body.key)?;
    let cipher = teacher_keys::cipher(&state)?;
    let replaced = store::replace(&state.pool, cipher, id, key)
        .await
        .map_err(teacher_keys::add_error)?
        .ok_or_else(ApiError::not_found)?;
    accounts::audit(
        &state.pool,
        cu.user.id,
        "system_key_replaced",
        &replaced.provider,
        None,
    )
    .await?;
    let usage = store::usage_by_key(&state.pool, USAGE_DAYS).await?;
    Ok(Json(item(replaced, &usage)))
}

/// 刪除一把系統 key；該 key 的用量紀錄一併刪除。刪光目前模型供應商的 key 後，系統用途自動回到 `.apikeylist`。
#[utoipa::path(
    delete,
    path = "/api/admin/system-ai/keys/{id}",
    tag = "admin",
    summary = "刪除系統 key",
    params(("id" = Uuid, Path)),
    responses((status = 204, description = "已刪除"), crate::error::CommonErrors)
)]
pub(crate) async fn delete_key(
    RequireAdmin(cu): RequireAdmin,
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = teacher_keys::parse_id(&raw_id)?;
    let mut tx = state.pool.begin().await?;
    let provider = store::delete(&mut *tx, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    accounts::audit(&mut *tx, cu.user.id, "system_key_deleted", &provider, None).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 測試一把系統 key：只呼叫供應商的模型清單端點、不送對話內容。可用就清掉先前記下的失效／額度／限流狀態。
#[utoipa::path(
    post,
    path = "/api/admin/system-ai/keys/{id}/test",
    tag = "admin",
    summary = "測試系統 key 是否可用",
    params(("id" = Uuid, Path)),
    responses((status = 200, body = KeyTestResult), crate::error::CommonErrors)
)]
pub(crate) async fn test_key(
    RequireAdmin(cu): RequireAdmin,
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
) -> Result<Json<KeyTestResult>, ApiError> {
    let id = teacher_keys::parse_id(&raw_id)?;
    let cipher = teacher_keys::cipher(&state)?;
    let Some((provider_id, key)) = store::get_secret(&state.pool, cipher, id).await? else {
        // 解不開（加密金鑰已輪替掉）的 key 算連線失敗，不當成不存在
        return if store::exists(&state.pool, id).await? {
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
            store::set_status_after(&state.pool, &key, KeyStatus::Ok, None).await?;
            "usable"
        }
        KeyCheck::ConnectionFailed => "connection_failed",
    };
    Ok(Json(KeyTestResult { result }))
}
