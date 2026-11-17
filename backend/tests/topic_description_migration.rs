//! F-32.1：既有題目的 `description` 轉成引導問題（S-04.8 2026-11 修訂，migration 只往前）。

use sqlx::{PgPool, migrate::Migrator};
use std::borrow::Cow;
use uuid::Uuid;

const MIGRATION: i64 = 20261117000000;

async fn insert(pool: &PgPool, title: &str, description: &str) -> Uuid {
    sqlx::query_scalar("INSERT INTO topics (title, description) VALUES ($1, $2) RETURNING id")
        .bind(title)
        .bind(description)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn questions(pool: &PgPool, id: Uuid) -> Vec<String> {
    sqlx::query_scalar("SELECT guiding_questions FROM topics WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = false)]
async fn description_becomes_guiding_questions_without_losing_the_column(pool: PgPool) {
    let all = sqlx::migrate!();
    let before = Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|m| m.version < MIGRATION)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    };
    before.run(&pool).await.unwrap();

    let lines = insert(&pool, "多行", "  為什麼？ \r\n\n  你怎麼想  \n").await;
    let empty = insert(&pool, "空白", "   \n ").await;
    let none = insert(&pool, "無內容", "").await;
    let seven = insert(&pool, "七行", "1\n2\n3\n4\n5\n6\n7").await;
    let sentence = |c: char| format!("{}。", c.to_string().repeat(89));
    let long_text = format!("{}{}{}", sentence('甲'), sentence('乙'), sentence('丙'));
    let sentences = insert(&pool, "長段落", &long_text).await;
    let uncut = "丁".repeat(300);
    let truncated = insert(&pool, "無標點長段落", &uncut).await;
    let exact = "戊".repeat(200);
    let exact_id = insert(&pool, "剛好兩百字", &exact).await;
    let filled = insert(&pool, "已填引導問題", "舊內容").await;
    sqlx::query("UPDATE topics SET guiding_questions = ARRAY['已手動填寫'] WHERE id = $1")
        .bind(filled)
        .execute(&pool)
        .await
        .unwrap();

    all.run(&pool).await.unwrap();
    all.run(&pool).await.unwrap();

    assert_eq!(questions(&pool, lines).await, ["為什麼？", "你怎麼想"]);
    assert!(questions(&pool, empty).await.is_empty());
    assert!(questions(&pool, none).await.is_empty());
    assert_eq!(questions(&pool, seven).await, ["1", "2", "3", "4", "5"]);
    assert_eq!(
        questions(&pool, sentences).await,
        [sentence('甲'), sentence('乙'), sentence('丙')]
    );
    assert_eq!(questions(&pool, truncated).await, ["丁".repeat(200)]);
    assert_eq!(questions(&pool, exact_id).await, [exact]);
    assert_eq!(questions(&pool, filled).await, ["已手動填寫"]);

    let kept: String = sqlx::query_scalar("SELECT description FROM topics WHERE id = $1")
        .bind(seven)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(kept, "1\n2\n3\n4\n5\n6\n7", "description 欄位原樣保留");
}
