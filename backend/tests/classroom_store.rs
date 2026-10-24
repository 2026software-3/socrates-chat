//! F-28.1 classroom data access integration tests (synthetic accounts only).

use socrates_chat_backend::classroom_store::{self, ClassroomStoreError};
use sqlx::PgPool;

#[sqlx::test]
async fn classroom_store_scopes_by_owner_and_database_enforces_normalized_unique_names(
    pool: PgPool,
) {
    let owner: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (email) VALUES ('store-owner@example.com') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let other: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (email) VALUES ('store-other@example.com') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let room = classroom_store::create(&pool, owner, "Research Room", None, None)
        .await
        .unwrap();
    assert_eq!(
        classroom_store::list(&pool, owner, false)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        classroom_store::list(&pool, other, false)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        classroom_store::list(&pool, other, true)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        classroom_store::get_visible(&pool, room.id, other, false)
            .await
            .unwrap()
            .is_none()
    );

    let duplicate = classroom_store::create(&pool, other, "  research room ", None, None).await;
    assert!(matches!(duplicate, Err(ClassroomStoreError::NameTaken)));
}

#[sqlx::test]
async fn classroom_store_database_index_serializes_concurrent_duplicate_names(pool: PgPool) {
    let owner: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (email) VALUES ('concurrent-owner@example.com') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let (first, second) = tokio::join!(
        classroom_store::create(&pool, owner, "  Concurrent Room  ", None, None),
        classroom_store::create(&pool, owner, "concurrent room", None, None),
    );

    let outcomes = [first, second];
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Err(ClassroomStoreError::NameTaken)))
            .count(),
        1
    );
}

#[sqlx::test]
async fn database_rejects_classroom_fields_over_the_length_limits(pool: PgPool) {
    let owner: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (email) VALUES ('limit-owner@example.com') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    for (name, year_label, description) in [
        ("x".repeat(256), None, None),
        ("Room".to_owned(), Some("y".repeat(101)), None),
        ("Room".to_owned(), None, Some("d".repeat(2001))),
    ] {
        let result = sqlx::query(
            "INSERT INTO classrooms (name, year_label, description, created_by)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(&name)
        .bind(&year_label)
        .bind(&description)
        .bind(owner)
        .execute(&pool)
        .await;
        let error = result.expect_err("over-limit classroom row must be rejected");
        assert_eq!(
            error
                .as_database_error()
                .and_then(|db| db.code())
                .as_deref(),
            Some("23514"),
            "expected a check violation"
        );
    }

    sqlx::query(
        "INSERT INTO classrooms (name, year_label, description, created_by)
         VALUES ($1, $2, $3, $4)",
    )
    .bind("名".repeat(255))
    .bind("年".repeat(100))
    .bind("說".repeat(2000))
    .bind(owner)
    .execute(&pool)
    .await
    .expect("rows at the limits are accepted");
}
