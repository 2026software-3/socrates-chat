//! F-30.2（S-02.2）：日誌不得包含學號。以真正的 tracing 輸出擷取，走過匯入、以學號登入（成功與失敗）、
//! 名單、學生帳號、總結列表與 `/api/me`。全部使用合成資料。
//! 這個檔案只能有一個測試：tracing 的全域訂閱者整個程序只能設定一次。

mod common;

use std::{
    io,
    sync::{Arc, Mutex},
};

use axum::{Router, body::Body, http::Response, http::StatusCode};
use common::*;
use serde_json::json;
use sqlx::PgPool;
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

/// 只用學號登入的合成學生；匯入與登入都用小寫，確認大小寫兩種寫法都不會進日誌。
const STUDENT_NUMBER: &str = "31415926C";
/// `seed_actors` 匯入的預設學生的合成學號。
const SEEDED_NUMBER: &str = "10000000A";

async fn login(app: &Router, account: &str, password: &str) -> Response<Body> {
    send(
        app,
        json_req(
            "POST",
            "/api/auth/login",
            None,
            Some(json!({"account": account, "password": password})),
        ),
    )
    .await
}

#[sqlx::test]
async fn logs_never_contain_student_numbers(pool: PgPool) {
    let logs = Captured::default();
    socrates_chat_backend::logging::json_subscriber()
        .with_writer(logs.clone())
        .with_max_level(tracing::Level::TRACE)
        .init();

    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let lower = STUDENT_NUMBER.to_lowercase();

    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/classrooms/{}/roster/import", a.classroom),
            Some(&a.teacher),
            Some(json!({"text": format!("日誌學生,{lower}")})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let temp = json_body(res).await["credentials"][0]["temporary_password"]
        .as_str()
        .unwrap()
        .to_string();

    // 以學號登入：先失敗一次，再成功（錯誤密碼每次執行都產生新的，不在原始碼寫死密碼值）
    let wrong_password = format!("Wrong-{}", uuid::Uuid::new_v4());
    let res = login(&app, STUDENT_NUMBER, &wrong_password).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let res = login(&app, &lower, &temp).await;
    assert_eq!(res.status(), StatusCode::OK);
    let cookie = session_cookie(&res).unwrap();
    // 臨時密碼的 session 仍可呼叫 /api/me，回應裡有自己的學號
    let me = json_body(send(&app, get("/api/me", Some(&cookie))).await).await;
    assert_eq!(me["student_number"], STUDENT_NUMBER);

    // 教師與管理者讀取帶學號的列表
    for cookie in [&a.teacher, &a.admin] {
        for path in [
            format!("/api/classrooms/{}/roster", a.classroom),
            format!("/api/classrooms/{}/students", a.classroom),
            "/api/summaries".to_string(),
        ] {
            let res = send(&app, get(&path, Some(cookie))).await;
            assert_eq!(res.status(), StatusCode::OK, "{path}");
        }
    }

    let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(
        output.contains("request completed") && output.contains("/api/auth/login"),
        "擷取到請求日誌，下面的斷言才有意義"
    );
    let seeded_lower = SEEDED_NUMBER.to_lowercase();
    // 失敗訊息只印名稱，不印值
    for (name, secret) in [
        ("學號（大寫）", STUDENT_NUMBER),
        ("學號（小寫）", lower.as_str()),
        ("預設學生學號（大寫）", SEEDED_NUMBER),
        ("預設學生學號（小寫）", seeded_lower.as_str()),
    ] {
        assert!(!output.contains(secret), "日誌不應包含：{name}");
    }
}
