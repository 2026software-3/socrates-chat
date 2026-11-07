//! F-29.1 的資料搬移：已上線的資料庫（NTNU）在活動仍用 `activities.topic_id` 時就套用了
//! `20261002010000`，之後不能再改那個檔案，只能由後面的 migration 把資料搬到 `activity_topics`。
//! 需要 `DATABASE_URL` 指向可建立資料庫的 PostgreSQL（見 docker-compose.yml）。

use sqlx::{
    PgPool, Row,
    migrate::{Migration, Migrator},
};
use uuid::Uuid;

/// 把活動改成多題目的 migration 版本；在它之前的就是已上線資料庫的狀態。
const ACTIVITY_TOPICS_VERSION: i64 = 20261107000000;

fn migrator(keep: impl Fn(&Migration) -> bool) -> Migrator {
    let all = sqlx::migrate!("./migrations");
    Migrator::with_migrations(all.iter().filter(|m| keep(m)).cloned().collect())
}

async fn insert_id(pool: &PgPool, sql: &'static str, bind: &[&str]) -> Uuid {
    let mut query = sqlx::query(sql);
    for value in bind {
        query = query.bind(value.to_string());
    }
    query.fetch_one(pool).await.unwrap().get("id")
}

#[sqlx::test(migrations = false)]
async fn existing_activities_keep_their_topic_as_the_first_one(pool: PgPool) {
    // 已上線的狀態：只套用到多題目 migration 之前，活動只有一個 topic_id
    migrator(|m| m.version < ACTIVITY_TOPICS_VERSION)
        .run(&pool)
        .await
        .unwrap();
    let teacher = insert_id(
        &pool,
        "INSERT INTO users (email) VALUES ($1) RETURNING id",
        &["teacher@example.test"],
    )
    .await;
    let classroom: Uuid = sqlx::query(
        "INSERT INTO classrooms (name, created_by) VALUES ('合成教室', $1) RETURNING id",
    )
    .bind(teacher)
    .fetch_one(&pool)
    .await
    .unwrap()
    .get("id");
    let trolley = insert_id(
        &pool,
        "INSERT INTO topics (title) VALUES ($1) RETURNING id",
        &["電車難題"],
    )
    .await;
    let free_will = insert_id(
        &pool,
        "INSERT INTO topics (title) VALUES ($1) RETURNING id",
        &["自由意志"],
    )
    .await;
    let mut activities = Vec::new();
    for (title, topic) in [
        ("活動甲", trolley),
        ("活動乙", trolley),
        ("活動丙", free_will),
    ] {
        let id: Uuid = sqlx::query(
            "INSERT INTO activities (title, topic_id, classroom_id) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(title)
        .bind(topic)
        .bind(classroom)
        .fetch_one(&pool)
        .await
        .unwrap()
        .get("id");
        activities.push((id, topic));
    }

    // 部署：套用全部 migration（已套用的不會重跑，也不能因舊檔被改而 checksum 不符）
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();

    let rows = sqlx::query("SELECT activity_id, topic_id, position FROM activity_topics")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(rows.len(), activities.len());
    for (activity, topic) in activities {
        let row = rows
            .iter()
            .find(|r| r.get::<Uuid, _>("activity_id") == activity)
            .expect("每個既有活動都要有一筆題目關聯");
        assert_eq!(row.get::<Uuid, _>("topic_id"), topic);
        assert_eq!(row.get::<i32, _>("position"), 0);
    }
    let old_column: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.columns \
         WHERE table_name = 'activities' AND column_name = 'topic_id'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(old_column, 0, "activities.topic_id 應已移除");
}

#[sqlx::test(migrations = false)]
async fn a_fresh_database_ends_with_the_same_activity_topics_shape(pool: PgPool) {
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_name = 'activity_topics' ORDER BY column_name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        columns,
        ["activity_id", "created_at", "position", "topic_id"]
    );
    let index: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_indexes WHERE indexname = 'activity_topics_topic_idx'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(index, 1);
}
