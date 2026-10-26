//! F-28.2 修課名單資料存取層整合測試（只使用合成資料）。

use socrates_chat_backend::{classroom_store, roster_store};
use sqlx::PgPool;
use uuid::Uuid;

async fn user(pool: &PgPool, email: &str) -> Uuid {
    sqlx::query_scalar("INSERT INTO users (email) VALUES ($1) RETURNING id")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn classroom(pool: &PgPool, owner: Uuid, name: &str) -> Uuid {
    classroom_store::create(pool, owner, name, None, None)
        .await
        .unwrap()
        .id
}

#[sqlx::test]
async fn roster_is_isolated_per_classroom_and_one_email_can_join_several(pool: PgPool) {
    let owner = user(&pool, "owner@example.com").await;
    let a = classroom(&pool, owner, "A").await;
    let b = classroom(&pool, owner, "B").await;
    let mut conn = pool.acquire().await.unwrap();

    assert!(
        roster_store::add(&mut conn, a, "s@example.com")
            .await
            .unwrap()
    );
    assert!(
        !roster_store::add(&mut conn, a, "s@example.com")
            .await
            .unwrap(),
        "adding the same email to the same classroom is a no-op"
    );
    assert!(
        roster_store::add(&mut conn, b, "s@example.com")
            .await
            .unwrap(),
        "the same email can also join another classroom"
    );
    assert!(
        roster_store::add(&mut conn, b, "only-b@example.com")
            .await
            .unwrap()
    );

    assert_eq!(
        roster_store::list_emails(&pool, a, None, 200)
            .await
            .unwrap(),
        ["s@example.com"]
    );
    assert_eq!(
        roster_store::list_emails(&pool, b, None, 200)
            .await
            .unwrap(),
        ["only-b@example.com", "s@example.com"]
    );
}

#[sqlx::test]
async fn removing_from_one_classroom_keeps_the_others(pool: PgPool) {
    let owner = user(&pool, "owner@example.com").await;
    let a = classroom(&pool, owner, "A").await;
    let b = classroom(&pool, owner, "B").await;
    let mut conn = pool.acquire().await.unwrap();
    roster_store::add(&mut conn, a, "s@example.com")
        .await
        .unwrap();
    roster_store::add(&mut conn, b, "s@example.com")
        .await
        .unwrap();

    assert!(
        roster_store::remove(&mut conn, a, "S@Example.com")
            .await
            .unwrap()
    );
    assert!(
        !roster_store::remove(&mut conn, a, "s@example.com")
            .await
            .unwrap(),
        "removing someone who is not on this roster reports nothing removed"
    );
    assert!(
        roster_store::list_emails(&pool, a, None, 200)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        roster_store::list_emails(&pool, b, None, 200)
            .await
            .unwrap(),
        ["s@example.com"]
    );
    assert!(
        roster_store::is_enrolled_anywhere(&pool, "s@example.com")
            .await
            .unwrap()
    );

    roster_store::remove(&mut conn, b, "s@example.com")
        .await
        .unwrap();
    assert!(
        !roster_store::is_enrolled_anywhere(&pool, "s@example.com")
            .await
            .unwrap()
    );
}

#[sqlx::test]
async fn students_list_is_scoped_to_one_classroom_with_one_account_per_email(pool: PgPool) {
    let owner = user(&pool, "owner@example.com").await;
    let student = user(&pool, "s@example.com").await;
    let a = classroom(&pool, owner, "A").await;
    let b = classroom(&pool, owner, "B").await;
    let mut conn = pool.acquire().await.unwrap();
    roster_store::add(&mut conn, a, "s@example.com")
        .await
        .unwrap();
    roster_store::add(&mut conn, a, "pending@example.com")
        .await
        .unwrap();
    roster_store::add(&mut conn, b, "s@example.com")
        .await
        .unwrap();

    let in_a = roster_store::list_students(&pool, a, None, 200, None)
        .await
        .unwrap();
    assert_eq!(
        in_a.iter().map(|s| s.email.as_str()).collect::<Vec<_>>(),
        ["pending@example.com", "s@example.com"]
    );
    assert_eq!(in_a[0].id, None);
    assert!(!in_a[0].has_account);
    assert_eq!(in_a[1].id, Some(student));
    assert!(in_a[1].has_account);
    assert_eq!(
        roster_store::list_students(&pool, b, None, 200, None)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[sqlx::test]
async fn database_requires_an_existing_classroom_for_roster_rows(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap();
    let result = roster_store::add(&mut conn, Uuid::new_v4(), "s@example.com").await;
    assert!(result.is_err());
}
