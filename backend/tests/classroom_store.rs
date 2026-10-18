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
