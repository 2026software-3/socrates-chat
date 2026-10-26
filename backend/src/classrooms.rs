//! 教室建立、檢視與編輯（F-28.1；S-14）。

use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
    routing::get,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    AppState,
    accounts::{
        DeleteSpec, audit, clean_reason, confirmation, delete_with_audit, ensure_confirmed,
    },
    auth::{CurrentUser, RequireTeacher},
    catalog::nullable,
    classroom_store::{self, Classroom, ClassroomChanges, ClassroomStoreError},
    error::ApiError,
};

const MAX_CLASSROOM_NAME_CHARS: usize = 255;
const MAX_CLASSROOM_YEAR_LABEL_CHARS: usize = 100;
const MAX_CLASSROOM_DESCRIPTION_CHARS: usize = 2_000;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/classrooms",
            get(list_classrooms).post(create_classroom),
        )
        .route(
            "/api/classrooms/{id}",
            get(get_classroom)
                .patch(update_classroom)
                .delete(delete_classroom),
        )
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct NewClassroom {
    #[schema(max_length = 255)]
    name: Option<String>,
    #[serde(default)]
    #[schema(max_length = 100)]
    year_label: Option<String>,
    #[serde(default)]
    #[schema(max_length = 2000)]
    description: Option<String>,
}

/// PATCH 欄位：未提供時保留原值，`null` 可清除選填欄位。
#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct ClassroomPatch {
    is_archived: Option<bool>,
    /// 封存或復原的選填原因，寫入稽核日誌
    #[schema(max_length = 500)]
    reason: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    #[schema(max_length = 255)]
    name: Option<Option<String>>,
    #[serde(default, deserialize_with = "nullable")]
    #[schema(max_length = 100)]
    year_label: Option<Option<String>>,
    #[serde(default, deserialize_with = "nullable")]
    #[schema(max_length = 2000)]
    description: Option<Option<String>>,
}

fn normalized_name(name: Option<String>) -> Result<String, ApiError> {
    name.map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .filter(|value| value.chars().count() <= MAX_CLASSROOM_NAME_CHARS)
        .ok_or(ApiError::bad_request(
            "invalid_classroom_name",
            "教室名稱不可為空或超過 255 個字元",
        ))
}

fn normalized_optional_field(
    value: Option<String>,
    max_chars: usize,
    code: &'static str,
    message: &'static str,
) -> Result<Option<String>, ApiError> {
    let value = value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if value
        .as_ref()
        .is_some_and(|value| value.chars().count() > max_chars)
    {
        return Err(ApiError::bad_request(code, message));
    }
    Ok(value)
}

fn map_store_error(error: ClassroomStoreError) -> ApiError {
    match error {
        ClassroomStoreError::NameTaken => {
            ApiError::conflict("classroom_name_taken", "教室名稱已被使用")
        }
        ClassroomStoreError::Database(error) => error.into(),
    }
}

fn parse_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(raw).map_err(|_| ApiError::not_found())
}

/// 教師端清單的 `?classroom_id=` 篩選；沒帶代表操作者可管理的所有教室。
#[derive(Deserialize)]
pub(crate) struct ClassroomFilter {
    classroom_id: Option<String>,
}

/// 解析 `?classroom_id=`：有帶時必須是操作者可管理的教室（否則與不存在相同的 404）。
pub(crate) async fn filtered_classroom(
    state: &AppState,
    cu: &CurrentUser,
    filter: &ClassroomFilter,
) -> Result<Option<Uuid>, ApiError> {
    match &filter.classroom_id {
        Some(raw) => Ok(Some(visible_classroom(state, cu, raw).await?.id)),
        None => Ok(None),
    }
}

/// 路徑上的教室，必須是操作者可管理的（建立者或管理者）；
/// 格式不正確、不存在與他人的教室都回相同的 404，不外露教室是否存在。
pub(crate) async fn visible_classroom(
    state: &AppState,
    cu: &CurrentUser,
    raw_id: &str,
) -> Result<Classroom, ApiError> {
    classroom_store::get_visible(&state.pool, parse_id(raw_id)?, cu.user.id, cu.roles.admin)
        .await?
        .ok_or_else(ApiError::not_found)
}

/// 列出可管理的教室：教師只看自己建立的，管理者看全部。
#[utoipa::path(
    get,
    path = "/api/classrooms",
    tag = "classrooms",
    summary = "列出可管理的教室",
    responses((status = 200, description = "教室列表", body = Vec<Classroom>), crate::error::CommonErrors)
)]
pub(crate) async fn list_classrooms(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
) -> Result<Json<Vec<Classroom>>, ApiError> {
    Ok(Json(
        classroom_store::list(&state.pool, cu.user.id, cu.roles.admin).await?,
    ))
}

/// 教師建立教室；管理者可管理所有教室，但教室建立者需為教師。
#[utoipa::path(
    post,
    path = "/api/classrooms",
    tag = "classrooms",
    summary = "建立教室",
    request_body = NewClassroom,
    responses((status = 201, description = "已建立", body = Classroom), (status = 400, description = "invalid_classroom_name、invalid_classroom_year_label、invalid_classroom_description", body = crate::error::ErrorBody), (status = 409, description = "classroom_archived / classroom_name_taken", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn create_classroom(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Json(body): Json<NewClassroom>,
) -> Result<(StatusCode, Json<Classroom>), ApiError> {
    if !cu.roles.teacher {
        return Err(ApiError::forbidden("forbidden", "只有教師可建立教室"));
    }
    let name = normalized_name(body.name)?;
    let year_label = normalized_optional_field(
        body.year_label,
        MAX_CLASSROOM_YEAR_LABEL_CHARS,
        "invalid_classroom_year_label",
        "年度標籤不可超過 100 個字元",
    )?;
    let description = normalized_optional_field(
        body.description,
        MAX_CLASSROOM_DESCRIPTION_CHARS,
        "invalid_classroom_description",
        "說明不可超過 2,000 個字元",
    )?;
    let classroom = classroom_store::create(
        &state.pool,
        cu.user.id,
        &name,
        year_label.as_deref(),
        description.as_deref(),
    )
    .await
    .map_err(map_store_error)?;
    Ok((StatusCode::CREATED, Json(classroom)))
}

/// 查看教室；不存在與其他教師建立的教室都回相同 404。
#[utoipa::path(
    get,
    path = "/api/classrooms/{id}",
    tag = "classrooms",
    summary = "查看教室",
    params(("id" = Uuid, Path, description = "教室 ID")),
    responses((status = 200, description = "教室", body = Classroom), crate::error::CommonErrors)
)]
pub(crate) async fn get_classroom(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
) -> Result<Json<Classroom>, ApiError> {
    visible_classroom(&state, &cu, &raw_id).await.map(Json)
}

/// 編輯教室；管理者可編輯任一教室，教師只能編輯自己建立的教室。
#[utoipa::path(
    patch,
    path = "/api/classrooms/{id}",
    tag = "classrooms",
    summary = "編輯教室",
    params(("id" = Uuid, Path, description = "教室 ID")),
    request_body = ClassroomPatch,
    responses((status = 200, description = "已更新", body = Classroom), (status = 400, description = "invalid_classroom_name、invalid_classroom_year_label、invalid_classroom_description", body = crate::error::ErrorBody), (status = 409, description = "classroom_archived / classroom_name_taken", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn update_classroom(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
    Json(body): Json<ClassroomPatch>,
) -> Result<Json<Classroom>, ApiError> {
    let id = parse_id(&raw_id)?;
    let name = body.name.map(normalized_name).transpose()?;
    let year_label = body
        .year_label
        .map(|value| {
            normalized_optional_field(
                value,
                MAX_CLASSROOM_YEAR_LABEL_CHARS,
                "invalid_classroom_year_label",
                "年度標籤不可超過 100 個字元",
            )
        })
        .transpose()?;
    let description = body
        .description
        .map(|value| {
            normalized_optional_field(
                value,
                MAX_CLASSROOM_DESCRIPTION_CHARS,
                "invalid_classroom_description",
                "說明不可超過 2,000 個字元",
            )
        })
        .transpose()?;
    let reason = body.reason;
    let changes = ClassroomChanges {
        is_archived: body.is_archived,
        name,
        year_label,
        description,
    };
    // 鎖住教室列後在同一個交易內更新並寫稽核，封存狀態是否改變以鎖定後的值為準
    let mut tx = state.pool.begin().await?;
    let was_archived: bool =
        sqlx::query_scalar("SELECT is_archived FROM classrooms WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(ApiError::not_found)?;
    if was_archived
        && (changes.name.is_some() || changes.year_label.is_some() || changes.description.is_some())
    {
        return Err(ApiError::conflict("classroom_archived", "教室已封存"));
    }
    let updated =
        classroom_store::update_visible(&mut *tx, id, cu.user.id, cu.roles.admin, changes)
            .await
            .map_err(map_store_error)?
            .ok_or_else(ApiError::not_found)?;
    if updated.is_archived != was_archived {
        let action = if updated.is_archived {
            "classroom_archived"
        } else {
            "classroom_restored"
        };
        audit(
            &mut *tx,
            cu.user.id,
            action,
            &updated.name,
            clean_reason(reason).as_deref(),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(updated))
}

/// 刪除教室的確認：必須與教室名稱相同（S-14）。
#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct DeleteClassroomBody {
    confirm_name: String,
    /// 選填原因，寫入稽核日誌
    #[schema(max_length = 500)]
    reason: Option<String>,
}

/// 永久刪除教室；封存的教室可直接刪除，不必先復原。
#[utoipa::path(
    delete,
    path = "/api/classrooms/{id}",
    tag = "classrooms",
    summary = "永久刪除教室，連同活動、修課名單、活動對話與總結（不可復原）",
    params(("id" = Uuid, Path, description = "教室 ID")),
    request_body = DeleteClassroomBody,
    responses((status = 204, description = "已刪除"), (status = 400, description = "confirmation_mismatch", body = crate::error::ErrorBody), crate::error::CommonErrors)
)]
pub(crate) async fn delete_classroom(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
    body: Result<Json<DeleteClassroomBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    // 先確認教室存在且可管理（否則 404），再檢查確認欄位
    let classroom = visible_classroom(&state, &cu, &raw_id).await?;
    let body = confirmation(body)?;
    ensure_confirmed(&body.confirm_name, &classroom.name)?;
    // 名單、活動與其中的對話、總結由外鍵連動刪除；個人練習、點數與成就不受影響
    delete_with_audit(
        &state.pool,
        &DeleteSpec {
            lock: "SELECT id FROM classrooms WHERE id = $1 FOR UPDATE",
            count_conversations: "SELECT count(*) FROM conversations WHERE classroom_id = $1",
            delete: "DELETE FROM classrooms WHERE id = $1",
            action: "classroom_deleted",
        },
        classroom.id,
        cu.user.id,
        &classroom.name,
        body.reason,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Archived classrooms are read-only. Personal practice has no classroom.
pub(crate) async fn ensure_writable<'e>(
    executor: impl sqlx::Executor<'e, Database = sqlx::Postgres>,
    id: Uuid,
) -> Result<(), ApiError> {
    let archived: bool =
        sqlx::query_scalar("SELECT is_archived FROM classrooms WHERE id = $1 FOR SHARE")
            .bind(id)
            .fetch_one(executor)
            .await?;
    if archived {
        Err(ApiError::conflict("classroom_archived", "教室已封存"))
    } else {
        Ok(())
    }
}

pub(crate) async fn ensure_conversation_writable(
    state: &AppState,
    user: Uuid,
    id: Uuid,
) -> Result<(), ApiError> {
    let conv = crate::chat::owned_conversation(state, user, id).await?;
    if let Some(room) = conv.classroom_id {
        ensure_writable(&state.pool, room).await?;
    }
    Ok(())
}
