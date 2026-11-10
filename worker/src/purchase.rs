//! F-19.4：角色部件扣點與管理者定價。正式商品與素材待 S-07.1 決定。
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, patch, post},
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{
    AppState, achievements,
    auth::{RequireAdmin, RequireStudent},
    error::ApiError,
    points::{self, Entry},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/parts", get(list))
        .route("/api/parts/{id}/purchase", post(buy))
        .route("/api/admin/parts", get(admin_list))
        .route("/api/admin/parts/{id}", patch(update))
}

#[derive(Clone, Debug, FromRow, Serialize, utoipa::ToSchema)]
pub(crate) struct Part {
    id: String,
    name_zh_tw: String,
    name_en: String,
    name_es: String,
    price: i32,
    enabled: bool,
}

#[derive(FromRow, Serialize, utoipa::ToSchema)]
pub(crate) struct PartView {
    id: String,
    name_zh_tw: String,
    name_en: String,
    name_es: String,
    price: i32,
    enabled: bool,
    owned: bool,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PartUpdate {
    price: i32,
    enabled: bool,
}

#[utoipa::path(get, path = "/api/parts", tag = "parts", summary = "可兌換及已擁有的部件", responses((status = 200, body = [PartView])))]
pub(crate) async fn list(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
) -> Result<Json<Vec<PartView>>, ApiError> {
    let rows = sqlx::query_as("SELECT p.id, p.name_zh_tw, p.name_en, p.name_es, p.price, p.enabled, EXISTS (SELECT 1 FROM point_ledger l WHERE l.user_id=$1 AND l.source_type='purchase' AND l.source_id=p.id) AS owned FROM store_parts p WHERE p.enabled OR EXISTS (SELECT 1 FROM point_ledger l WHERE l.user_id=$1 AND l.source_type='purchase' AND l.source_id=p.id) ORDER BY p.id")
        .bind(cu.user.id).fetch_all(&state.pool).await?;
    Ok(Json(rows))
}

#[utoipa::path(post, path = "/api/parts/{id}/purchase", tag = "parts", summary = "購買部件", params(("id" = String, Path)), responses((status = 200, body = PartView), (status = 409, body = crate::error::ErrorBody)))]
pub(crate) async fn buy(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<PartView>, ApiError> {
    let mut tx = state.pool.begin().await?;
    // 鎖定學生，讓並行購買無法花費同一筆餘額；鎖定商品，確保價格與啟用狀態一致。
    sqlx::query("SELECT id FROM users WHERE id=$1")
        .bind(cu.user.id)
        .execute(&mut *tx)
        .await?;
    let part: Part = sqlx::query_as(
        "SELECT id, name_zh_tw, name_en, name_es, price, enabled FROM store_parts WHERE id=$1",
    )
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(ApiError::not_found)?;
    if !part.enabled {
        return Err(ApiError::not_found());
    }
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM point_ledger WHERE user_id=$1 AND source_type='purchase' AND source_id=$2)")
        .bind(cu.user.id).bind(&id).fetch_one(&mut *tx).await?;
    if exists {
        return Err(ApiError::conflict("part_owned", "已擁有此部件"));
    }
    if points::balance(&mut tx, cu.user.id).await? < i64::from(part.price) {
        return Err(ApiError::conflict("insufficient_points", "點數不足"));
    }
    let entry = Entry::purchase(cu.user.id, "part_purchase", &part.id, part.price as u32);
    if !points::record(&mut tx, &entry).await? {
        return Err(ApiError::conflict("part_owned", "已擁有此部件"));
    }
    achievements::record_first_character_part_in_tx(&mut tx, cu.user.id).await?;
    tx.commit().await?;
    Ok(Json(PartView {
        id: part.id,
        name_zh_tw: part.name_zh_tw,
        name_en: part.name_en,
        name_es: part.name_es,
        price: part.price,
        enabled: part.enabled,
        owned: true,
    }))
}

#[utoipa::path(get, path = "/api/admin/parts", tag = "parts", summary = "部件設定（管理者）", responses((status = 200, body = [Part])))]
pub(crate) async fn admin_list(
    RequireAdmin(_): RequireAdmin,
    State(state): State<AppState>,
) -> Result<Json<Vec<Part>>, ApiError> {
    Ok(Json(
        sqlx::query_as(
            "SELECT id, name_zh_tw, name_en, name_es, price, enabled FROM store_parts ORDER BY id",
        )
        .fetch_all(&state.pool)
        .await?,
    ))
}

#[utoipa::path(patch, path = "/api/admin/parts/{id}", tag = "parts", summary = "修改部件價格與狀態（管理者）", params(("id" = String, Path)), request_body = PartUpdate, responses((status = 200, body = Part), (status = 400, body = crate::error::ErrorBody)))]
pub(crate) async fn update(
    RequireAdmin(cu): RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PartUpdate>,
) -> Result<Json<Part>, ApiError> {
    if body.price < 0 {
        return Err(ApiError::bad_request("invalid_request", "價格不可為負"));
    }
    let mut tx = state.pool.begin().await?;
    let old: Part = sqlx::query_as(
        "SELECT id, name_zh_tw, name_en, name_es, price, enabled FROM store_parts WHERE id=$1",
    )
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(ApiError::not_found)?;
    let new: Part = sqlx::query_as("UPDATE store_parts SET price=$2, enabled=$3 WHERE id=$1 RETURNING id, name_zh_tw, name_en, name_es, price, enabled")
        .bind(&id).bind(body.price).bind(body.enabled).fetch_one(&mut *tx).await?;
    sqlx::query("INSERT INTO audit_logs (actor_id, action, target_email, old_value, new_value) VALUES ($1, 'part_updated', $2, $3, $4)")
        .bind(cu.user.id).bind(&id).bind(sqlx::types::Json(&old)).bind(sqlx::types::Json(&new)).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(new))
}
