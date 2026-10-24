//! F-28.3 活動與對話歸屬的資料存取層整合測試（只使用合成資料）。

use socrates_chat_backend::{activity_store, classroom_store, roster_store};
use sqlx::PgPool;
use uuid::Uuid;

async fn user(pool: &PgPool, email: &str) -> Uuid {
    sqlx::query_scalar("INSERT INTO users (email) VALUES ($1) RETURNING id")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn room(pool: &PgPool, owner: Uuid, name: &str) -> Uuid {
    classroom_store::create(pool, owner, name, None, None)
        .await
        .unwrap()
        .id
}

async fn topic(pool: &PgPool) -> Uuid {
    sqlx::query_scalar("INSERT INTO topics (title) VALUES ('題目') RETURNING id")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn teacher_queries_go_through_their_own_classrooms(pool: PgPool) {
    let (t1, t2) = (
        user(&pool, "t1@example.com").await,
        user(&pool, "t2@example.com").await,
    );
    let (a, b) = (room(&pool, t1, "A").await, room(&pool, t2, "B").await);
    let topic = topic(&pool).await;
    let in_a = activity_store::create(&pool, a, t1, "甲", "", topic)
        .await
        .unwrap();
    let in_b = activity_store::create(&pool, b, t2, "乙", "", topic)
        .await
        .unwrap();

    // 兩個教室引用同一題目
    assert_eq!(in_a.topic_id, in_b.topic_id);
    let topics: i64 = sqlx::query_scalar("SELECT count(*) FROM topics")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(topics, 1);

    let mine = activity_store::list(&pool, t1, false, None, None, 200)
        .await
        .unwrap();
    assert_eq!(mine.iter().map(|x| x.id).collect::<Vec<_>>(), [in_a.id]);
    assert_eq!(
        activity_store::list(&pool, t1, true, None, None, 200)
            .await
            .unwrap()
            .len(),
        2
    );
    assert!(
        activity_store::get_visible(&pool, in_b.id, t1, false)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        activity_store::get_visible(&pool, in_b.id, t1, true)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        activity_store::list(&pool, t1, false, Some(b), None, 200)
            .await
            .unwrap()
            .is_empty()
    );
}

#[sqlx::test]
async fn a_student_only_gets_published_activities_of_classrooms_they_are_on(pool: PgPool) {
    let owner = user(&pool, "t@example.com").await;
    let (a, b) = (room(&pool, owner, "A").await, room(&pool, owner, "B").await);
    let topic = topic(&pool).await;
    let in_a = activity_store::create(&pool, a, owner, "甲", "", topic)
        .await
        .unwrap();
    let in_b = activity_store::create(&pool, b, owner, "乙", "", topic)
        .await
        .unwrap();
    sqlx::query("UPDATE activities SET status = 'published'")
        .execute(&pool)
        .await
        .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    roster_store::add(&mut conn, a, "s@example.com")
        .await
        .unwrap();

    let seen = activity_store::list_published_for_student(&pool, "s@example.com")
        .await
        .unwrap();
    assert_eq!(seen.iter().map(|x| x.id).collect::<Vec<_>>(), [in_a.id]);
    assert!(
        activity_store::find_startable(&mut conn, in_b.id, "s@example.com")
            .await
            .unwrap()
            .is_none()
    );
    let ok = activity_store::find_startable(&mut conn, in_a.id, "s@example.com")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ok.classroom_id, a);

    roster_store::add(&mut conn, b, "s@example.com")
        .await
        .unwrap();
    assert_eq!(
        activity_store::list_published_for_student(&pool, "s@example.com")
            .await
            .unwrap()
            .len(),
        2
    );
}

#[sqlx::test]
async fn conversation_classroom_survives_the_activity_and_blocks_classroom_deletion(pool: PgPool) {
    let owner = user(&pool, "t@example.com").await;
    let student = user(&pool, "s@example.com").await;
    let a = room(&pool, owner, "A").await;
    let topic = topic(&pool).await;
    let activity = activity_store::create(&pool, a, owner, "甲", "", topic)
        .await
        .unwrap();
    let conv: Uuid = sqlx::query_scalar(
        "INSERT INTO conversations (user_id, activity_id, classroom_id, title) VALUES ($1, $2, $3, 't') RETURNING id",
    )
    .bind(student).bind(activity.id).bind(a)
    .fetch_one(&pool).await.unwrap();

    sqlx::query("DELETE FROM activities WHERE id = $1")
        .bind(activity.id)
        .execute(&pool)
        .await
        .unwrap();
    let kept: Option<Uuid> =
        sqlx::query_scalar("SELECT classroom_id FROM conversations WHERE id = $1")
            .bind(conv)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(kept, Some(a));

    // 教室不能刪除（有對話引用時資料庫拒絕）
    assert!(
        sqlx::query("DELETE FROM classrooms WHERE id = $1")
            .bind(a)
            .execute(&pool)
            .await
            .is_err()
    );
}

#[sqlx::test]
async fn personal_practice_never_matches_a_classroom_scoped_query(pool: PgPool) {
    let owner = user(&pool, "t@example.com").await;
    let student = user(&pool, "s@example.com").await;
    let a = room(&pool, owner, "A").await;
    let practice: Uuid = sqlx::query_scalar(
        "INSERT INTO conversations (user_id, title) VALUES ($1, '練習') RETURNING id",
    )
    .bind(student)
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO summaries (conversation_id, status, stance, reasons) VALUES ($1, 'ready', 's', 'r')",
    )
    .bind(practice).execute(&pool).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    roster_store::add(&mut conn, a, "s@example.com")
        .await
        .unwrap();

    assert_eq!(
        roster_store::count_completed(&pool, a, "s@example.com")
            .await
            .unwrap(),
        0
    );
    assert!(
        classroom_store::get_visible(&pool, a, owner, false)
            .await
            .unwrap()
            .is_some()
    );
}
