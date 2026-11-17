//! 教室頁的「已加入」人數（S-14）以 `audit_logs` 裡的 `login` 紀錄判斷學生是否登入過，
//! 每次登入都會新增一列，所以依操作者查登入紀錄必須走索引，不能隨紀錄量全表掃描。
//! 需要 `DATABASE_URL` 指向可建立資料庫的 PostgreSQL（見 docker-compose.yml）。

use sqlx::PgPool;

#[sqlx::test]
async fn login_lookup_by_actor_can_use_an_index(pool: PgPool) {
    let mut tx = pool.begin().await.unwrap();
    // 測試資料量太小，planner 本來就會選 seq scan；關掉後只有存在可用的索引才會走 index scan
    sqlx::query("SET LOCAL enable_seqscan = off")
        .execute(&mut *tx)
        .await
        .unwrap();

    let plan: Vec<String> = sqlx::query_scalar(
        "EXPLAIN SELECT 1 FROM audit_logs \
         WHERE actor_id = '00000000-0000-0000-0000-000000000001' AND action = 'login'",
    )
    .fetch_all(&mut *tx)
    .await
    .unwrap();

    let plan = plan.join("\n");
    assert!(
        plan.contains("Index Scan") || plan.contains("Index Only Scan") || plan.contains("Bitmap"),
        "依 actor_id 查登入紀錄沒有走索引：\n{plan}"
    );
}
