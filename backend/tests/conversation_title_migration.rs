//! 對話標題補正：活動對話的 `conversations.title` 應是題目名稱，但舊客戶端只傳 `activity_id` 時，
//! 曾把活動名稱存成標題（教師的 `topic` 篩選與主張分組都以標題區分題目）。
//! NTNU 已上線，不能改舊 migration，由往前的 migration 補正既有資料。
//! 需要 `DATABASE_URL` 指向可建立資料庫的 PostgreSQL（見 docker-compose.yml）。

use sqlx::{
    PgPool, Row,
    migrate::{Migration, Migrator},
};
use uuid::Uuid;

/// 補正標題的 migration 版本；在它之前就是已上線資料庫的狀態。
const TITLE_FIX_VERSION: i64 = 20261109000100;

fn migrator(keep: impl Fn(&Migration) -> bool) -> Migrator {
    let all = sqlx::migrate!("./migrations");
    Migrator::with_migrations(all.iter().filter(|m| keep(m)).cloned().collect())
}

async fn insert_id(pool: &PgPool, sql: &'static str, bind: &[Uuid], text: &[&str]) -> Uuid {
    let mut query = sqlx::query(sql);
    for id in bind {
        query = query.bind(*id);
    }
    for value in text {
        query = query.bind(value.to_string());
    }
    query.fetch_one(pool).await.unwrap().get("id")
}

async fn user(pool: &PgPool, email: &str) -> Uuid {
    insert_id(
        pool,
        "INSERT INTO users (email) VALUES ($1) RETURNING id",
        &[],
        &[email],
    )
    .await
}

async fn conversation(
    pool: &PgPool,
    user: Uuid,
    activity: Option<Uuid>,
    topic: Option<Uuid>,
    classroom: Option<Uuid>,
    title: &str,
) -> Uuid {
    sqlx::query(
        "INSERT INTO conversations (user_id, activity_id, topic_id, classroom_id, title) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(user)
    .bind(activity)
    .bind(topic)
    .bind(classroom)
    .bind(title)
    .fetch_one(pool)
    .await
    .unwrap()
    .get("id")
}

async fn title_of(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT title FROM conversations WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = false)]
async fn activity_conversations_titled_with_the_activity_name_get_the_topic_name(pool: PgPool) {
    migrator(|m| m.version < TITLE_FIX_VERSION)
        .run(&pool)
        .await
        .unwrap();

    let teacher = user(&pool, "teacher@example.test").await;
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
        &[],
        &["電車難題"],
    )
    .await;
    let activity: Uuid = sqlx::query(
        "INSERT INTO activities (title, classroom_id) VALUES ('活動甲', $1) RETURNING id",
    )
    .bind(classroom)
    .fetch_one(&pool)
    .await
    .unwrap()
    .get("id");
    sqlx::query("INSERT INTO activity_topics (activity_id, topic_id, position) VALUES ($1, $2, 0)")
        .bind(activity)
        .bind(trolley)
        .execute(&pool)
        .await
        .unwrap();

    let legacy = user(&pool, "legacy@example.test").await;
    let already = user(&pool, "already@example.test").await;
    let renamed = user(&pool, "renamed@example.test").await;
    let orphan = user(&pool, "orphan@example.test").await;
    let practice = user(&pool, "practice@example.test").await;

    // 舊客戶端只傳 activity_id：標題是活動名稱 → 要補成題目名稱
    let legacy_conv = conversation(
        &pool,
        legacy,
        Some(activity),
        Some(trolley),
        Some(classroom),
        "活動甲",
    )
    .await;
    // 標題已經是題目名稱 → 不動
    let already_conv = conversation(
        &pool,
        already,
        Some(activity),
        Some(trolley),
        Some(classroom),
        "電車難題",
    )
    .await;
    // 標題既不是目前的活動名稱也不是題目名稱（例如活動曾改名）→ 無法確定來源，不動
    let renamed_conv = conversation(
        &pool,
        renamed,
        Some(activity),
        Some(trolley),
        Some(classroom),
        "活動甲（舊名）",
    )
    .await;
    // 題目已被刪除（topic_id 為空）→ 沒有題目名稱可補，不動
    let orphan_conv = conversation(
        &pool,
        orphan,
        Some(activity),
        None,
        Some(classroom),
        "活動甲",
    )
    .await;
    // 個人練習沒有活動 → 不動，即使標題與某個活動同名
    let practice_conv = conversation(&pool, practice, None, Some(trolley), None, "活動甲").await;

    // 部署：套用全部 migration（已套用的不會重跑，也不能因舊檔被改而 checksum 不符）
    let all = sqlx::migrate!("./migrations");
    all.run(&pool).await.unwrap();
    all.run(&pool).await.unwrap(); // 重跑不應有任何變化

    assert_eq!(title_of(&pool, legacy_conv).await, "電車難題");
    assert_eq!(title_of(&pool, already_conv).await, "電車難題");
    assert_eq!(title_of(&pool, renamed_conv).await, "活動甲（舊名）");
    assert_eq!(title_of(&pool, orphan_conv).await, "活動甲");
    assert_eq!(title_of(&pool, practice_conv).await, "活動甲");
}
