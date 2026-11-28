//! F-28.3 活動與對話歸屬的資料存取層整合測試（只使用合成資料）。

use socrates_chat_backend::{activity_store, classroom_store, roster_store};
use sqlx::PgPool;
use uuid::Uuid;

async fn user(pool: &PgPool, email: &str) -> Uuid {
    let student_id = match email {
        "s@example.com" => Some("14000001A"),
        "only-b@example.com" => Some("14000002A"),
        "pending@example.com" => Some("14000003A"),
        _ => None,
    };
    sqlx::query_scalar("INSERT INTO users (email, student_id, imported_name) VALUES ($1, $2, CASE WHEN $2::text IS NULL THEN NULL ELSE '合成姓名' END) RETURNING id")
        .bind(email)
        .bind(student_id)
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
    let in_a = activity_store::create_with_schedule(&pool, a, t1, "甲", "", &[topic], (None, None))
        .await
        .unwrap();
    let in_b = activity_store::create_with_schedule(&pool, b, t2, "乙", "", &[topic], (None, None))
        .await
        .unwrap();

    // 兩個教室引用同一題目
    assert_eq!(in_a.topic_id, in_b.topic_id);
    let topics: i64 = sqlx::query_scalar("SELECT count(*) FROM topics")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(topics, 1);

    let mine = activity_store::list(&pool, t1, false, None, false, None, 200)
        .await
        .unwrap();
    assert_eq!(mine.iter().map(|x| x.id).collect::<Vec<_>>(), [in_a.id]);
    assert_eq!(
        activity_store::list(&pool, t1, true, None, false, None, 200)
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
        activity_store::list(&pool, t1, false, Some(b), false, None, 200)
            .await
            .unwrap()
            .is_empty()
    );
}

#[sqlx::test]
async fn a_student_only_gets_published_activities_of_classrooms_they_are_on(pool: PgPool) {
    let student = user(&pool, "s@example.com").await;
    let owner = user(&pool, "t@example.com").await;
    let (a, b) = (room(&pool, owner, "A").await, room(&pool, owner, "B").await);
    let topic = topic(&pool).await;
    let in_a =
        activity_store::create_with_schedule(&pool, a, owner, "甲", "", &[topic], (None, None))
            .await
            .unwrap();
    let in_b =
        activity_store::create_with_schedule(&pool, b, owner, "乙", "", &[topic], (None, None))
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

    let seen = activity_store::list_published_for_student(&pool, student)
        .await
        .unwrap();
    assert_eq!(
        seen.iter().map(|x| x.activity.id).collect::<Vec<_>>(),
        [in_a.id]
    );
    assert!(
        activity_store::find_startable(&mut conn, in_b.id, student)
            .await
            .unwrap()
            .is_none()
    );
    let ok = activity_store::find_startable(&mut conn, in_a.id, student)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ok.classroom_id, a);

    roster_store::add(&mut conn, b, "s@example.com")
        .await
        .unwrap();
    assert_eq!(
        activity_store::list_published_for_student(&pool, student)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[sqlx::test]
async fn deleting_an_activity_or_classroom_deletes_its_conversations(pool: PgPool) {
    let owner = user(&pool, "t@example.com").await;
    let student = user(&pool, "s@example.com").await;
    let a = room(&pool, owner, "A").await;
    let topic = topic(&pool).await;
    let mut convs = Vec::new();
    let mut activities = Vec::new();
    for title in ["甲", "乙"] {
        let activity = activity_store::create_with_schedule(
            &pool,
            a,
            owner,
            title,
            "",
            &[topic],
            (None, None),
        )
        .await
        .unwrap();
        let conv: Uuid = sqlx::query_scalar(
            "INSERT INTO conversations (user_id, activity_id, classroom_id, title) VALUES ($1, $2, $3, 't') RETURNING id",
        )
        .bind(student).bind(activity.id).bind(a)
        .fetch_one(&pool).await.unwrap();
        convs.push(conv);
        activities.push(activity.id);
    }
    let remaining = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM conversations")
            .fetch_one(&pool)
            .await
            .unwrap()
    };

    // 刪除活動：只帶走該活動的對話（S-14）
    sqlx::query("DELETE FROM activities WHERE id = $1")
        .bind(activities[0])
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(remaining().await, 1);

    // 刪除教室：其中的活動與對話一併刪除；個人練習不受影響
    sqlx::query("INSERT INTO conversations (user_id, title) VALUES ($1, '練習')")
        .bind(student)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM classrooms WHERE id = $1")
        .bind(a)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(remaining().await, 1);
    let practice: i64 =
        sqlx::query_scalar("SELECT count(*) FROM conversations WHERE classroom_id IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(practice, 1);
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
        "INSERT INTO summaries (conversation_id, status, stance, reasons, completed_at) VALUES ($1, 'ready', 's', 'r', now())",
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

#[sqlx::test]
async fn every_activity_read_and_state_change_carries_translations(pool: PgPool) {
    let teacher = user(&pool, "t@example.com").await;
    let classroom = room(&pool, teacher, "A").await;
    let topic = topic(&pool).await;
    let created = activity_store::create_with_schedule(
        &pool,
        classroom,
        teacher,
        "甲",
        "",
        &[topic],
        (None, None),
    )
    .await
    .unwrap();
    sqlx::query(
        r#"UPDATE activities SET translations = '{"en":{"title":"Localized"}}' WHERE id = $1"#,
    )
    .bind(created.id)
    .execute(&pool)
    .await
    .unwrap();

    let english_title = |a: &activity_store::Activity| {
        serde_json::to_value(a).unwrap()["translations"]["en"]["title"].clone()
    };
    let visible = activity_store::get_visible(&pool, created.id, teacher, false)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(english_title(&visible), "Localized", "get_visible");
    let found = activity_store::find_by_id(&pool, created.id).await.unwrap();
    assert_eq!(english_title(&found), "Localized", "find_by_id");
    let published =
        activity_store::set_status_writable(&pool, created.id, teacher, false, "published")
            .await
            .unwrap()
            .unwrap();
    assert_eq!(
        english_title(&published),
        "Localized",
        "set_status_writable"
    );
    let archived = activity_store::set_archived(&pool, created.id, true)
        .await
        .unwrap();
    assert_eq!(english_title(&archived), "Localized", "set_archived");
}
