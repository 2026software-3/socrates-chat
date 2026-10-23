//! F-28.1 教室資料存取；每位教師只看得到自己建立的教室，管理者可看全部。

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct Classroom {
    pub id: Uuid,
    pub name: String,
    pub year_label: Option<String>,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug)]
pub enum ClassroomStoreError {
    NameTaken,
    Database(sqlx::Error),
}

impl From<sqlx::Error> for ClassroomStoreError {
    fn from(error: sqlx::Error) -> Self {
        if is_classroom_name_conflict(&error) {
            Self::NameTaken
        } else {
            Self::Database(error)
        }
    }
}

pub fn is_classroom_name_conflict(error: &sqlx::Error) -> bool {
    error.as_database_error().is_some_and(|db| {
        db.code().as_deref() == Some("23505")
            && db.constraint() == Some("classrooms_normalized_name_unique")
    })
}

pub async fn list(
    pool: &PgPool,
    actor_id: Uuid,
    is_admin: bool,
) -> Result<Vec<Classroom>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, name, year_label, description, created_at, updated_at
         FROM classrooms
         WHERE $1 OR created_by = $2
         ORDER BY lower(name), id",
    )
    .bind(is_admin)
    .bind(actor_id)
    .fetch_all(pool)
    .await
}

pub async fn get_visible(
    pool: &PgPool,
    id: Uuid,
    actor_id: Uuid,
    is_admin: bool,
) -> Result<Option<Classroom>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, name, year_label, description, created_at, updated_at
         FROM classrooms
         WHERE id = $1 AND ($2 OR created_by = $3)",
    )
    .bind(id)
    .bind(is_admin)
    .bind(actor_id)
    .fetch_optional(pool)
    .await
}

pub async fn create(
    pool: &PgPool,
    owner_id: Uuid,
    name: &str,
    year_label: Option<&str>,
    description: Option<&str>,
) -> Result<Classroom, ClassroomStoreError> {
    sqlx::query_as(
        "INSERT INTO classrooms (name, year_label, description, created_by)
         VALUES ($1, $2, $3, $4)
         RETURNING id, name, year_label, description, created_at, updated_at",
    )
    .bind(name)
    .bind(year_label)
    .bind(description)
    .bind(owner_id)
    .fetch_one(pool)
    .await
    .map_err(Into::into)
}

#[derive(Default)]
pub struct ClassroomChanges {
    pub name: Option<String>,
    pub year_label: Option<Option<String>>,
    pub description: Option<Option<String>>,
}

pub async fn update_visible(
    pool: &PgPool,
    id: Uuid,
    actor_id: Uuid,
    is_admin: bool,
    changes: ClassroomChanges,
) -> Result<Option<Classroom>, ClassroomStoreError> {
    if changes.name.is_none() && changes.year_label.is_none() && changes.description.is_none() {
        return get_visible(pool, id, actor_id, is_admin)
            .await
            .map_err(Into::into);
    }

    sqlx::query_as(
        "UPDATE classrooms
         SET name = COALESCE($4, name),
             year_label = CASE WHEN $5 THEN $6 ELSE year_label END,
             description = CASE WHEN $7 THEN $8 ELSE description END,
             updated_at = now()
         WHERE id = $1 AND ($2 OR created_by = $3)
         RETURNING id, name, year_label, description, created_at, updated_at",
    )
    .bind(id)
    .bind(is_admin)
    .bind(actor_id)
    .bind(changes.name)
    .bind(changes.year_label.is_some())
    .bind(changes.year_label.flatten())
    .bind(changes.description.is_some())
    .bind(changes.description.flatten())
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}
