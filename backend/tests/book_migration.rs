//! F-14.3 forward migration preserves existing topic/activity data and remains repeatable.
use socrates_chat_backend::{activity_store, book_store, classroom_store};
use sqlx::{PgPool, migrate::Migrator};
use uuid::Uuid;

#[sqlx::test(migrations = false)]
async fn books_migration_preserves_existing_activities_and_can_run_again(pool: PgPool) {
    let all = sqlx::migrate!();
    let legacy = Migrator::with_migrations(
        all.iter()
            .filter(|m| m.version < 20261120000000)
            .cloned()
            .collect(),
    );
    legacy.run(&pool).await.unwrap();
    let teacher: Uuid = sqlx::query_scalar(
        "INSERT INTO users(email) VALUES ('migration-teacher@example.com') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let room = classroom_store::create(&pool, teacher, "原有合成教室", None, None)
        .await
        .unwrap()
        .id;
    let first: Uuid =
        sqlx::query_scalar("INSERT INTO topics(title) VALUES ('原有第一題') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    let second: Uuid =
        sqlx::query_scalar("INSERT INTO topics(title) VALUES ('原有第二題') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    let activity = activity_store::create_with_schedule(
        &pool,
        room,
        teacher,
        "原有活動",
        "保留說明",
        &[first, second],
        (None, None),
    )
    .await
    .unwrap();
    all.run(&pool).await.unwrap();
    all.run(&pool).await.unwrap();
    let preserved = activity_store::find_by_id(&pool, activity.id)
        .await
        .unwrap();
    assert_eq!(preserved.title, "原有活動");
    assert_eq!(preserved.description, "保留說明");
    assert_eq!(preserved.topic_ids, [first, second]);
    assert!(
        book_store::activity_books(&pool, activity.id, first)
            .await
            .unwrap()
            .is_empty()
    );
    let mut tx = pool.begin().await.unwrap();
    book_store::sync_activity_topics(&mut tx, activity.id, &[second, first])
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        activity_store::find_by_id(&pool, activity.id)
            .await
            .unwrap()
            .topic_ids,
        [second, first]
    );
}

#[sqlx::test(migrations = false)]
async fn work_migration_preserves_each_legacy_edition_and_both_list_selections(pool: PgPool) {
    let all = sqlx::migrate!();
    let legacy = Migrator::with_migrations(
        all.iter()
            .filter(|m| m.version < 20261120000100)
            .cloned()
            .collect(),
    );
    legacy.run(&pool).await.unwrap();
    let teacher: Uuid = sqlx::query_scalar(
        "INSERT INTO users(email) VALUES ('works-migration@example.com') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let room = classroom_store::create(&pool, teacher, "合成教室", None, None)
        .await
        .unwrap()
        .id;
    let topic: Uuid =
        sqlx::query_scalar("INSERT INTO topics(title) VALUES ('合成題') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    let first:Uuid=sqlx::query_scalar("INSERT INTO books(title,authors,language,publication_year,isbn,source_url,description,recommendation_note,external_sources,created_by) VALUES ('相同合成書名','甲作者','zh-TW','2000','9780000000002','https://example.com/book','合成簡介','合成推薦','[{\"source\":\"google_books\",\"source_id\":\"synthetic-123\"}]',$1) RETURNING id").bind(teacher).fetch_one(&pool).await.unwrap();
    let second:Uuid=sqlx::query_scalar("INSERT INTO books(title,language,is_active) VALUES ('相同合成書名','en',false) RETURNING id").fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO topic_books(topic_id,book_id) VALUES ($1,$2),($1,$3)")
        .bind(topic)
        .bind(first)
        .bind(second)
        .execute(&pool)
        .await
        .unwrap();
    let activity = activity_store::create_with_schedule(
        &pool,
        room,
        teacher,
        "合成活動",
        "",
        &[topic],
        (None, None),
    )
    .await
    .unwrap();
    // A customized activity selection is independent of its topic default.
    sqlx::query("DELETE FROM activity_topic_books WHERE activity_id=$1")
        .bind(activity.id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO activity_topic_books(activity_id,topic_id,book_id) VALUES ($1,$2,$3)")
        .bind(activity.id)
        .bind(topic)
        .bind(second)
        .execute(&pool)
        .await
        .unwrap();
    all.run(&pool).await.unwrap();
    all.run(&pool).await.unwrap();
    let works = book_store::topic_books(&pool, topic).await.unwrap();
    assert_eq!(works.len(), 2); // Similar titles are never automatically grouped.
    for id in [first, second] {
        let work = book_store::get_work(&pool, id).await.unwrap().unwrap();
        assert_eq!(work.editions.len(), 1);
        assert_eq!(work.editions[0].id, id);
        assert_eq!(work.editions[0].work_id, id);
    }
    let active = book_store::get(&pool, first).await.unwrap().unwrap();
    assert_eq!(active.authors.as_deref(), Some("甲作者"));
    assert_eq!(active.created_by, Some(teacher));
    assert_eq!(active.language.as_deref(), Some("zh-TW"));
    assert_eq!(active.publication_year.as_deref(), Some("2000"));
    assert_eq!(active.isbn.as_deref(), Some("9780000000002"));
    assert_eq!(
        active.source_url.as_deref(),
        Some("https://example.com/book")
    );
    assert_eq!(active.description.as_deref(), Some("合成簡介"));
    assert_eq!(active.recommendation_note.as_deref(), Some("合成推薦"));
    assert_eq!(active.external_sources[0].source_id, "synthetic-123");
    assert!(
        !book_store::get(&pool, second)
            .await
            .unwrap()
            .unwrap()
            .is_active
    );
    let copied = book_store::activity_books(&pool, activity.id, topic)
        .await
        .unwrap();
    assert_eq!(copied.len(), 1);
    assert_eq!(copied[0].id, second);
}
