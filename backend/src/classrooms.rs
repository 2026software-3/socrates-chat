//! 教室建立、檢視與編輯（F-28.1；S-14）。

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use serde::{Deserialize, Deserializer};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{CurrentUser, RequireTeacher},
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
            get(get_classroom).patch(update_classroom),
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

fn nullable<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
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
    responses((status = 201, description = "已建立", body = Classroom), (status = 400, description = "invalid_classroom_name、invalid_classroom_year_label、invalid_classroom_description", body = crate::error::ErrorBody), (status = 409, description = "classroom_name_taken", body = crate::error::ErrorBody), crate::error::CommonErrors)
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
    let id = parse_id(&raw_id)?;
    classroom_store::get_visible(&state.pool, id, cu.user.id, cu.roles.admin)
        .await?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

/// 編輯教室；管理者可編輯任一教室，教師只能編輯自己建立的教室。
#[utoipa::path(
    patch,
    path = "/api/classrooms/{id}",
    tag = "classrooms",
    summary = "編輯教室",
    params(("id" = Uuid, Path, description = "教室 ID")),
    request_body = ClassroomPatch,
    responses((status = 200, description = "已更新", body = Classroom), (status = 400, description = "invalid_classroom_name、invalid_classroom_year_label、invalid_classroom_description", body = crate::error::ErrorBody), (status = 409, description = "classroom_name_taken", body = crate::error::ErrorBody), crate::error::CommonErrors)
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
    let changes = ClassroomChanges {
        name,
        year_label,
        description,
    };
    classroom_store::update_visible(&state.pool, id, cu.user.id, cu.roles.admin, changes)
        .await
        .map_err(map_store_error)?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}
