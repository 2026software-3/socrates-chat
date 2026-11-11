//! S-09.3 就緒檢查失敗時的日誌：只記錯誤種類，不得帶出連線字串、帳密、主機或資料庫的錯誤文字。
//! 以真正的 tracing 輸出擷取（做法同 `log_hygiene.rs`）。不需要資料庫：連的是連不上的位址。
//! 這個檔案只能有一個測試：tracing 的全域訂閱者整個程序只能設定一次。

mod common;

use std::{
    io,
    sync::{Arc, Mutex},
};

use axum::http::StatusCode;
use common::*;
use sqlx::{PgPool, postgres::PgPoolOptions};
use tracing_subscriber::fmt::MakeWriter;

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

/// 兩條失敗路徑都要檢查：連不上（取連線逾時）與資料庫回錯誤（帳密錯誤，錯誤文字會帶出使用者名稱）。
/// `pool` 只用來取得測試資料庫的位址；實際被檢查的是兩個帶有可辨識帳密的 pool。
#[sqlx::test]
async fn failed_readiness_check_logs_the_failure_without_connection_details(pool: PgPool) {
    let logs = Captured::default();
    socrates_chat_backend::logging::json_subscriber()
        .with_writer(logs.clone())
        .with_max_level(tracing::Level::TRACE)
        .init();

    let unreachable = PgPoolOptions::new()
        .connect_lazy("postgres://leaky_user:leaky_pass@127.0.0.1:1/leaky_db")
        .unwrap();
    let rejected = PgPoolOptions::new().connect_lazy_with(
        (*pool.connect_options())
            .clone()
            .username("leaky_user")
            .password("leaky_pass")
            .database("leaky_db"),
    );

    for pool in [unreachable, rejected] {
        let res = send(&test_app(pool), get("/ready", None)).await;
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    // 擷取有效：兩次失敗都有被記錄下來
    assert_eq!(
        output.matches("readiness check failed").count(),
        2,
        "failures were not logged: {output}"
    );
    // 只記錯誤種類：逾時，以及帳密錯誤的 SQLSTATE（28P01），證明兩條路徑都走到了
    assert!(output.contains("timeout"), "{output}");
    assert!(output.contains("28P01"), "{output}");
    let lower = output.to_lowercase();
    for leaked in [
        "leaky_user",
        "leaky_pass",
        "leaky_db",
        "127.0.0.1",
        "postgres://",
        // 資料庫／連線的錯誤文字（sqlx::Error 的 Display）
        "refused",
        "os error",
        "error communicating",
        "error returned from database",
        "password authentication",
    ] {
        assert!(!lower.contains(leaked), "log leaks `{leaked}`: {output}");
    }
}
