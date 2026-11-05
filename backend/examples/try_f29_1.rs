//! Run from backend/: cargo run --example try_f29_1
//!
//! F-29.1 驗證腳本：驗證活動多題目資料模型、排序保存與更新。

use socrates_chat_backend::activity_store;
use sqlx::postgres::PgPoolOptions;
use std::{env, error::Error};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("--------------------------------------------------");
    println!("F-29.1 Verification: Multiple Topics for Activities");
    println!("--------------------------------------------------");

    let db_url = env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://socrates:socrates@127.0.0.1:5432/socrates".to_string());

    println!("Connecting to database: {db_url}");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await?;

    // 1. 取得或建立至少 2 個題目
    let mut topics: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, title FROM topics ORDER BY created_at ASC LIMIT 2")
            .fetch_all(&pool)
            .await?;

    if topics.is_empty() {
        let t: (Uuid, String) = sqlx::query_as(
            "INSERT INTO topics (title, description, category) VALUES ('電車難題', '你會拉桿嗎？', '倫理學') RETURNING id, title",
        )
        .fetch_one(&pool)
        .await?;
        topics.push(t);
    }
    if topics.len() < 2 {
        let t: (Uuid, String) = sqlx::query_as(
            "INSERT INTO topics (title, description, category) VALUES ('說謊可以嗎？', '任何情況都不能說謊嗎？', '倫理學') RETURNING id, title",
        )
        .fetch_one(&pool)
        .await?;
        topics.push(t);
    }

    println!("\n[1/5] Checking topics in catalog");
    println!("  - Topic A: {} ({})", topics[0].1, topics[0].0);
    println!("  - Topic B: {} ({})", topics[1].1, topics[1].0);

    // 2. 取得或建立測試教室與使用者
    let user_id: Uuid = match sqlx::query_scalar("SELECT id FROM users LIMIT 1")
        .fetch_optional(&pool)
        .await?
    {
        Some(id) => id,
        None => {
            sqlx::query_scalar(
                "INSERT INTO users (email, password_hash, display_name) VALUES ('test_teacher@example.com', 'dummy_hash', '測試教師') RETURNING id",
            )
            .fetch_one(&pool)
            .await?
        }
    };

    let classroom_id: Uuid = match sqlx::query_scalar(
        "SELECT id FROM classrooms WHERE NOT is_archived LIMIT 1",
    )
    .fetch_optional(&pool)
    .await?
    {
        Some(id) => id,
        None => sqlx::query_scalar(
            "INSERT INTO classrooms (name, created_by) VALUES ('哲學思辨一班', $1) RETURNING id",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await?,
    };

    println!("\n[2/5] Checking classroom");
    println!("  - Classroom ID: {classroom_id}");

    // 3. 建立包含「題目 B, 題目 A」的活動（刻意指定順序驗證排序保存）
    println!("\n[3/5] Creating activity with ordered topics: [Topic B, Topic A]");
    let desired_order = vec![topics[1].0, topics[0].0];
    let activity = activity_store::create_with_schedule(
        &pool,
        classroom_id,
        user_id,
        "F-29.1 實測：多題目哲學思辨",
        "本活動包含兩道哲學題目，請依序討論",
        &desired_order,
        (None, None),
    )
    .await?;

    println!("  - Activity ID: {}", activity.id);
    println!("  - Saved topic_ids: {:?}", activity.topic_ids);
    assert_eq!(
        activity.topic_ids, desired_order,
        "Order mismatch in activity creation"
    );
    println!("  [PASS] Topic ordering preserved on creation");

    // 4. 從資料庫重新查詢活動
    println!("\n[4/5] Fetching activity from database (find_by_id)");
    let fetched = activity_store::find_by_id(&pool, activity.id).await?;
    println!("  - Fetched topic_ids: {:?}", fetched.topic_ids);
    assert_eq!(fetched.topic_ids, desired_order);
    println!("  [PASS] Topic order verified from database records");

    // 5. 追加第 3 個題目並測試重排
    println!("\n[5/5] Appending Topic C and updating order to [Topic A, Topic B, Topic C]");
    let topic_c: (Uuid, String) = sqlx::query_as(
        "INSERT INTO topics (title, description, category) VALUES ('自由意志存在嗎？', '我們是否有選擇的自由？', '形上學') RETURNING id, title",
    )
    .fetch_one(&pool)
    .await?;
    println!("  - Topic C: {} ({})", topic_c.1, topic_c.0);

    let new_order = vec![topics[0].0, topics[1].0, topic_c.0];

    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM activity_topics WHERE activity_id = $1")
        .bind(activity.id)
        .execute(&mut *tx)
        .await?;
    for (pos, tid) in new_order.iter().enumerate() {
        sqlx::query(
            "INSERT INTO activity_topics (activity_id, topic_id, position) VALUES ($1, $2, $3)",
        )
        .bind(activity.id)
        .bind(tid)
        .bind(pos as i32)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    let updated = activity_store::find_by_id(&pool, activity.id).await?;
    println!("  - Updated topic_ids: {:?}", updated.topic_ids);
    assert_eq!(updated.topic_ids, new_order);
    println!("  [PASS] Update and reordering verified");

    println!("\n--------------------------------------------------");
    println!("F-29.1 verification finished successfully.");
    println!("--------------------------------------------------");

    Ok(())
}
