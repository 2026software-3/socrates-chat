//! Confirmed work/edition identity and lifecycle, using only synthetic records.
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test]
async fn creating_an_edition_atomically_creates_its_work(pool: PgPool) {
    let id: Uuid = sqlx::query_scalar("INSERT INTO books(title) VALUES ('合成版本') RETURNING id")
        .fetch_one(&pool)
        .await
        .unwrap();
    let (work_id, title, active): (Uuid, String, bool) = sqlx::query_as(
        "SELECT w.id,w.title,w.is_active FROM books b JOIN book_works w ON w.id=b.work_id WHERE b.id=$1")
        .bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(work_id, id);
    assert_eq!(title, "合成版本");
    assert!(active);
}

use socrates_chat_backend::book_store::{self, BookData};
async fn edition(pool: &PgPool, title: &str) -> book_store::Book {
    book_store::create(
        pool,
        None,
        &BookData {
            title: title.into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
}
async fn topic(pool: &PgPool) -> Uuid {
    sqlx::query_scalar("INSERT INTO topics(title) VALUES ('合成題目') RETURNING id")
        .fetch_one(pool)
        .await
        .unwrap()
}
async fn select(pool: &PgPool, topic: Uuid, works: &[Uuid]) {
    let mut tx = pool.begin().await.unwrap();
    assert!(
        book_store::replace_topic_books(&mut tx, topic, works)
            .await
            .unwrap()
    );
    tx.commit().await.unwrap();
}
#[sqlx::test]
async fn merge_redirects_and_deduplicates_lists_and_preserves_editions(pool: PgPool) {
    let a = edition(&pool, "原作品版本").await;
    let b = edition(&pool, "保留作品版本").await;
    let t = topic(&pool).await;
    select(&pool, t, &[a.work_id, b.work_id]).await;
    let teacher: Uuid = sqlx::query_scalar(
        "INSERT INTO users(email) VALUES ('work-merge@example.com') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let room =
        socrates_chat_backend::classroom_store::create(&pool, teacher, "合成教室", None, None)
            .await
            .unwrap()
            .id;
    let activity = socrates_chat_backend::activity_store::create_with_schedule(
        &pool,
        room,
        teacher,
        "合成活動",
        "",
        &[t],
        (None, None),
    )
    .await
    .unwrap();
    assert_eq!(
        book_store::activity_books(&pool, activity.id, t)
            .await
            .unwrap()
            .len(),
        2
    );
    // A separate topic and activity list remain independent of the merged list.
    let other_topic = topic(&pool).await;
    select(&pool, other_topic, &[b.work_id]).await;
    let mut tx = pool.begin().await.unwrap();
    assert!(
        book_store::merge_works(&mut tx, a.work_id, b.work_id)
            .await
            .unwrap()
    );
    tx.commit().await.unwrap();
    assert!(
        book_store::get_work(&pool, a.work_id)
            .await
            .unwrap()
            .is_none()
    );
    let list = book_store::topic_books(&pool, t).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, b.work_id);
    assert_eq!(list[0].editions.len(), 2);
    let activity_list = book_store::activity_books(&pool, activity.id, t)
        .await
        .unwrap();
    assert_eq!(activity_list.len(), 1);
    assert_eq!(activity_list[0].id, b.work_id);
    assert_eq!(
        book_store::topic_books(&pool, other_topic).await.unwrap()[0].id,
        b.work_id
    );
    assert_eq!(
        book_store::get(&pool, a.id).await.unwrap().unwrap().title,
        "原作品版本"
    );
}
#[sqlx::test]
async fn move_final_edition_preserves_source_work_and_does_not_add_target_to_lists(pool: PgPool) {
    let a = edition(&pool, "移動版本").await;
    let b = edition(&pool, "目的版本").await;
    let t = topic(&pool).await;
    select(&pool, t, &[a.work_id]).await;
    let mut tx = pool.begin().await.unwrap();
    assert_eq!(
        book_store::move_edition(&mut tx, a.id, b.work_id)
            .await
            .unwrap()
            .unwrap()
            .work_id,
        b.work_id
    );
    tx.commit().await.unwrap();
    let list = book_store::topic_books(&pool, t).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, a.work_id);
    assert!(list[0].editions.is_empty());
    assert_eq!(
        book_store::get_work(&pool, b.work_id)
            .await
            .unwrap()
            .unwrap()
            .editions
            .len(),
        2
    );
}
#[sqlx::test]
async fn deletion_removes_work_only_for_final_edition_and_flags_are_independent(pool: PgPool) {
    let a = edition(&pool, "第一版").await;
    let b = edition(&pool, "第二版").await;
    let t = topic(&pool).await;
    select(&pool, t, &[a.work_id]).await;
    let mut tx = pool.begin().await.unwrap();
    book_store::merge_works(&mut tx, b.work_id, a.work_id)
        .await
        .unwrap();
    book_store::set_work_active(&mut tx, a.work_id, false)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        book_store::get(&pool, a.id)
            .await
            .unwrap()
            .unwrap()
            .is_active
    );
    let mut tx = pool.begin().await.unwrap();
    book_store::set_work_active(&mut tx, a.work_id, true)
        .await
        .unwrap();
    book_store::set_active(&mut *tx, a.id, false).await.unwrap();
    tx.commit().await.unwrap();
    let w = book_store::get_work(&pool, a.work_id)
        .await
        .unwrap()
        .unwrap();
    assert!(w.is_active);
    assert_eq!(w.editions.iter().filter(|e| e.is_active).count(), 1);
    assert!(!book_store::impact(&pool, a.id).await.unwrap().deletes_work);
    book_store::delete(&pool, a.id).await.unwrap();
    assert_eq!(
        book_store::topic_books(&pool, t).await.unwrap()[0]
            .editions
            .len(),
        1
    );
    let impact = book_store::impact(&pool, b.id).await.unwrap();
    assert!(impact.deletes_work);
    assert_eq!(impact.topic_count, 1);
    book_store::delete(&pool, b.id).await.unwrap();
    assert!(
        book_store::get_work(&pool, a.work_id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(book_store::topic_books(&pool, t).await.unwrap().is_empty());
}
#[sqlx::test]
async fn deleting_whole_work_deletes_all_editions_and_list_links(pool: PgPool) {
    let a = edition(&pool, "第一版本").await;
    let b = edition(&pool, "第二版本").await;
    let t = topic(&pool).await;
    select(&pool, t, &[a.work_id]).await;
    let mut tx = pool.begin().await.unwrap();
    book_store::merge_works(&mut tx, b.work_id, a.work_id)
        .await
        .unwrap();
    assert!(book_store::delete_work(&mut tx, a.work_id).await.unwrap());
    tx.commit().await.unwrap();
    assert!(book_store::get(&pool, a.id).await.unwrap().is_none());
    assert!(book_store::get(&pool, b.id).await.unwrap().is_none());
    assert!(book_store::topic_books(&pool, t).await.unwrap().is_empty());
}

#[sqlx::test]
async fn inactive_or_empty_work_can_be_retained_but_not_added_to_new_lists(pool: PgPool) {
    let a = edition(&pool, "可移動版本").await;
    let b = edition(&pool, "保留版本").await;
    let first = topic(&pool).await;
    let second = topic(&pool).await;
    select(&pool, first, &[a.work_id]).await;
    let mut tx = pool.begin().await.unwrap();
    book_store::set_active(&mut *tx, a.id, false).await.unwrap();
    assert!(
        !book_store::replace_topic_books(&mut tx, second, &[a.work_id])
            .await
            .unwrap()
    );
    assert!(
        book_store::replace_topic_books(&mut tx, first, &[a.work_id])
            .await
            .unwrap()
    );
    book_store::move_edition(&mut tx, a.id, b.work_id)
        .await
        .unwrap();
    assert!(
        !book_store::replace_topic_books(&mut tx, second, &[a.work_id])
            .await
            .unwrap()
    );
    assert!(
        book_store::replace_topic_books(&mut tx, first, &[a.work_id])
            .await
            .unwrap()
    );
    book_store::set_work_active(&mut tx, b.work_id, false)
        .await
        .unwrap();
    assert!(
        !book_store::replace_topic_books(&mut tx, second, &[b.work_id])
            .await
            .unwrap()
    );
    tx.commit().await.unwrap();
}
