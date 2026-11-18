//! `scripts/seed-demo.sh` 建立的示範教室必須能直接用 AI（F-40.4-DB 之後新建教室預設不是過渡中，
//! 沒有教師 key 就會停用 AI；示範教師沒有 key，所以示範教室標為過渡中、沿用系統 key）。
use socrates_chat_backend::demo;
use sqlx::PgPool;

/// 取出腳本裡第一段 `<<'SQL'` … `SQL`（建立教室、名單與題目）。
fn classroom_block() -> String {
    let script = include_str!("../../scripts/seed-demo.sh");
    let start = script.find("<<'SQL'\n").expect("first SQL block") + "<<'SQL'\n".len();
    let end = start + script[start..].find("\nSQL\n").expect("end of SQL block");
    script[start..end].to_owned()
}

#[sqlx::test]
async fn demo_classrooms_keep_using_the_system_key(pool: PgPool) {
    demo::seed_accounts(&pool).await.unwrap();
    // 內容是版本控制中的腳本本身，不含任何外部輸入
    sqlx::raw_sql(sqlx::AssertSqlSafe(classroom_block()))
        .execute(&pool)
        .await
        .unwrap();
    let rows: Vec<(String, bool)> = sqlx::query_as(
        "SELECT name, ai_transitional FROM classrooms WHERE description LIKE '[demo]%' ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 3);
    for (name, transitional) in rows {
        assert!(transitional, "{name} 應為過渡中");
    }
}
