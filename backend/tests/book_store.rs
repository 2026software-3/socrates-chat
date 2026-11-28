//! F-14.3 database behavior: synthetic books, independent per-topic lists and cascades.
use sqlx::PgPool;

#[sqlx::test]
async fn a_book_can_be_created_with_only_a_title(pool: PgPool) {
    let (title, author, active): (String, Option<String>, bool) = sqlx::query_as(
        "INSERT INTO books (title) VALUES ('合成書目') RETURNING title, authors, is_active",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(title, "合成書目");
    assert_eq!(author, None);
    assert!(active);
}
use socrates_chat_backend::{
    activity_store,
    book_store::{self, BookData},
    classroom_store,
};
use uuid::Uuid;

fn data(title: &str) -> BookData {
    BookData {
        title: title.to_owned(),
        ..Default::default()
    }
}
async fn owner(pool: &PgPool) -> Uuid {
    sqlx::query_scalar("INSERT INTO users(email) VALUES ('book-owner@example.com') RETURNING id")
        .fetch_one(pool)
        .await
        .unwrap()
}
async fn topic(pool: &PgPool, title: &str) -> Uuid {
    sqlx::query_scalar("INSERT INTO topics(title) VALUES($1) RETURNING id")
        .bind(title)
        .fetch_one(pool)
        .await
        .unwrap()
}
async fn defaults(pool: &PgPool, topic_id: Uuid, ids: &[Uuid]) {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM topics WHERE id=$1 FOR UPDATE")
        .bind(topic_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(
        book_store::replace_topic_books(&mut tx, topic_id, ids)
            .await
            .unwrap()
    );
    tx.commit().await.unwrap();
}

#[sqlx::test]
async fn activity_topics_copy_defaults_once_and_reordering_keeps_custom_lists(pool: PgPool) {
    let teacher = owner(&pool).await;
    let room = classroom_store::create(&pool, teacher, "合成教室", None, None)
        .await
        .unwrap()
        .id;
    let t1 = topic(&pool, "公平").await;
    let t2 = topic(&pool, "自主").await;
    let t3 = topic(&pool, "責任").await;
    let a = book_store::create(&pool, Some(teacher), &data("甲書"))
        .await
        .unwrap();
    let b = book_store::create(&pool, Some(teacher), &data("乙書"))
        .await
        .unwrap();
    defaults(&pool, t1, &[a.id]).await;
    defaults(&pool, t2, &[b.id]).await;
    defaults(&pool, t3, &[a.id]).await;
    let activity = activity_store::create_with_schedule(
        &pool,
        room,
        teacher,
        "活動",
        "",
        &[t1, t2],
        (None, None),
    )
    .await
    .unwrap();
    assert_eq!(
        book_store::activity_books(&pool, activity.id, t1)
            .await
            .unwrap()[0]
            .id,
        a.id
    );
    assert_eq!(
        book_store::activity_books(&pool, activity.id, t2)
            .await
            .unwrap()[0]
            .id,
        b.id
    );
    defaults(&pool, t1, &[b.id]).await;
    assert_eq!(
        book_store::activity_books(&pool, activity.id, t1)
            .await
            .unwrap()[0]
            .id,
        a.id
    );
    let mut tx = pool.begin().await.unwrap();
    assert!(
        book_store::replace_activity_books(&mut tx, activity.id, t1, &[b.id])
            .await
            .unwrap()
    );
    book_store::sync_activity_topics(&mut tx, activity.id, &[t2, t1, t3])
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        book_store::activity_books(&pool, activity.id, t1)
            .await
            .unwrap()[0]
            .id,
        b.id
    );
    assert_eq!(
        book_store::activity_books(&pool, activity.id, t2)
            .await
            .unwrap()[0]
            .id,
        b.id
    );
    assert_eq!(
        book_store::activity_books(&pool, activity.id, t3)
            .await
            .unwrap()[0]
            .id,
        a.id
    );
    assert_eq!(
        activity_store::find_by_id(&pool, activity.id)
            .await
            .unwrap()
            .topic_ids,
        [t2, t1, t3]
    );
}

#[sqlx::test]
async fn disabled_books_stay_linked_but_cannot_join_new_lists(pool: PgPool) {
    let teacher = owner(&pool).await;
    let room = classroom_store::create(&pool, teacher, "合成教室", None, None)
        .await
        .unwrap()
        .id;
    let t1 = topic(&pool, "公平").await;
    let t2 = topic(&pool, "自主").await;
    let a = book_store::create(&pool, Some(teacher), &data("停用書"))
        .await
        .unwrap();
    defaults(&pool, t1, &[a.id]).await;
    let mut status_tx = pool.begin().await.unwrap();
    book_store::set_work_active(&mut status_tx, a.work_id, false)
        .await
        .unwrap();
    status_tx.commit().await.unwrap();
    assert!(!book_store::topic_books(&pool, t1).await.unwrap()[0].is_active);
    let mut tx = pool.begin().await.unwrap();
    assert!(
        book_store::replace_topic_books(&mut tx, t1, &[a.id])
            .await
            .unwrap()
    );
    assert!(
        !book_store::replace_topic_books(&mut tx, t2, &[a.id])
            .await
            .unwrap()
    );
    assert!(
        !book_store::replace_topic_books(&mut tx, t1, &[Uuid::new_v4()])
            .await
            .unwrap()
    );
    tx.commit().await.unwrap();
    assert_eq!(book_store::topic_books(&pool, t1).await.unwrap().len(), 1);
    let activity =
        activity_store::create_with_schedule(&pool, room, teacher, "活動", "", &[t1], (None, None))
            .await
            .unwrap();
    assert!(
        book_store::activity_books(&pool, activity.id, t1)
            .await
            .unwrap()
            .is_empty()
    );
    let mut status_tx = pool.begin().await.unwrap();
    book_store::set_work_active(&mut status_tx, a.work_id, true)
        .await
        .unwrap();
    status_tx.commit().await.unwrap();
    defaults(&pool, t2, &[a.id]).await;
}

#[sqlx::test]
async fn removal_and_resource_deletion_never_delete_shared_books(pool: PgPool) {
    let teacher = owner(&pool).await;
    let room = classroom_store::create(&pool, teacher, "合成教室", None, None)
        .await
        .unwrap()
        .id;
    let t1 = topic(&pool, "公平").await;
    let a = book_store::create(&pool, Some(teacher), &data("甲書"))
        .await
        .unwrap();
    defaults(&pool, t1, &[a.id]).await;
    let activity =
        activity_store::create_with_schedule(&pool, room, teacher, "活動", "", &[t1], (None, None))
            .await
            .unwrap();
    defaults(&pool, t1, &[]).await;
    assert_eq!(
        book_store::activity_books(&pool, activity.id, t1)
            .await
            .unwrap()
            .len(),
        1
    );
    sqlx::query("DELETE FROM classrooms WHERE id=$1")
        .bind(room)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        book_store::activity_books(&pool, activity.id, t1)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(book_store::get(&pool, a.id).await.unwrap().is_some());
    defaults(&pool, t1, &[a.id]).await;
    sqlx::query("DELETE FROM topics WHERE id=$1")
        .bind(t1)
        .execute(&pool)
        .await
        .unwrap();
    assert!(book_store::get(&pool, a.id).await.unwrap().is_some());
    sqlx::query("DELETE FROM users WHERE id=$1")
        .bind(teacher)
        .execute(&pool)
        .await
        .unwrap();
    let retained = book_store::get(&pool, a.id).await.unwrap().unwrap();
    assert!(retained.created_by.is_none());
}

#[sqlx::test]
async fn book_deletion_cascades_all_lists_and_owner_rules_apply_to_updates(pool: PgPool) {
    let teacher = owner(&pool).await;
    let room = classroom_store::create(&pool, teacher, "合成教室", None, None)
        .await
        .unwrap()
        .id;
    let t1 = topic(&pool, "公平").await;
    let a = book_store::create(&pool, Some(teacher), &data("甲書"))
        .await
        .unwrap();
    assert!(
        book_store::update_owned(&pool, a.id, Uuid::new_v4(), false, &data("不應更新"))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        book_store::update_owned(&pool, a.id, teacher, false, &data("自己更新"))
            .await
            .unwrap()
            .unwrap()
            .title,
        "自己更新"
    );
    assert_eq!(
        book_store::update_owned(&pool, a.id, Uuid::new_v4(), true, &data("管理者更新"))
            .await
            .unwrap()
            .unwrap()
            .title,
        "管理者更新"
    );
    defaults(&pool, t1, &[a.id]).await;
    let activity =
        activity_store::create_with_schedule(&pool, room, teacher, "活動", "", &[t1], (None, None))
            .await
            .unwrap();
    let impact = book_store::impact(&pool, a.id).await.unwrap();
    assert_eq!((impact.topic_count, impact.activity_topic_count), (1, 1));
    assert!(book_store::delete(&pool, a.id).await.unwrap());
    assert!(book_store::topic_books(&pool, t1).await.unwrap().is_empty());
    assert!(
        book_store::activity_books(&pool, activity.id, t1)
            .await
            .unwrap()
            .is_empty()
    );
}

#[sqlx::test]
async fn schema_limits_count_unicode_characters_not_bytes(pool: PgPool) {
    book_store::create(&pool, None, &data(&"書".repeat(255)))
        .await
        .unwrap();
    assert!(
        book_store::create(&pool, None, &data(&"書".repeat(256)))
            .await
            .is_err()
    );
    assert!(book_store::create(&pool, None, &data("   ")).await.is_err());
}
