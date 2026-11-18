//! 記用量失敗的日誌等級：key 在 AI 呼叫途中被管理者刪除時，寫入會因外鍵（SQLSTATE 23503）失敗，
//! 這是預期情況，只留 debug；其他失敗仍是 warn。全部使用合成資料。
//! 只有一個測試，訂閱者以 `set_default` 設定，避免互相干擾。

mod common;

use std::{
    io,
    sync::{Arc, Mutex},
};

use common::*;
use socrates_chat_backend::teacher_key_usage as usage;
use sqlx::PgPool;
use tracing_subscriber::fmt::MakeWriter;
use uuid::Uuid;

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

struct Sink(Arc<Mutex<Vec<u8>>>);

impl io::Write for Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Sink;
    fn make_writer(&'a self) -> Sink {
        Sink(self.0.clone())
    }
}

impl Captured {
    fn take(&self) -> Vec<serde_json::Value> {
        let text = String::from_utf8(std::mem::take(&mut *self.0.lock().unwrap())).unwrap();
        text.lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
}

#[sqlx::test]
async fn deleted_key_is_debug_and_other_failures_stay_warn(pool: PgPool) {
    let teacher = seed_user(&pool, "t@example.com").await;
    let classroom: Uuid = sqlx::query_scalar(
        "INSERT INTO classrooms (name, created_by) VALUES ('c', $1) RETURNING id",
    )
    .bind(teacher)
    .fetch_one(&pool)
    .await
    .unwrap();
    // 真的觸發外鍵錯誤：key 不存在（等同記用量前剛被刪除）
    let fk_error = usage::record(&pool, Uuid::new_v4(), classroom, "openai", "m", None)
        .await
        .unwrap_err();

    let logs = Captured::default();
    let _guard = tracing::subscriber::set_default(
        socrates_chat_backend::logging::json_subscriber()
            .with_writer(logs.clone())
            .with_max_level(tracing::Level::DEBUG)
            .finish(),
    );

    usage::log_record_failure(&fk_error);
    let lines = logs.take();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["level"], "DEBUG");
    assert!(
        lines[0]["fields"]["message"]
            .as_str()
            .unwrap()
            .contains("key was deleted"),
        "{lines:?}"
    );
    assert_eq!(lines[0]["fields"]["class"], "23503");

    usage::log_record_failure(&sqlx::Error::PoolTimedOut);
    let lines = logs.take();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["level"], "WARN");
    assert_eq!(lines[0]["fields"]["message"], "failed to record AI usage");
}
