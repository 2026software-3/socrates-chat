//! F-36 / S-17：客訴與寄信。全部使用合成資料；寄信對象是測試內的假 SMTP 伺服器。

mod common;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::http::StatusCode;
use base64::{Engine, engine::general_purpose::STANDARD};
use common::*;
use serde_json::{Value, json};
use socrates_chat_backend::{
    AppState,
    mail::{Security, SmtpConfig},
};
use sqlx::PgPool;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};

const SUBMIT: &str = "/api/complaints";
const ADMIN_LIST: &str = "/api/admin/complaints";

async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let res = send(app, json_req(method, path, cookie, body)).await;
    let status = res.status();
    let value = if status == StatusCode::NO_CONTENT {
        Value::Null
    } else {
        json_body(res).await
    };
    (status, value)
}

async fn submit(app: &axum::Router, cookie: &str, body: Value) -> (StatusCode, Value) {
    call(app, "POST", SUBMIT, Some(cookie), Some(body)).await
}

fn complaint(description: &str) -> Value {
    json!({"category": "connection", "description": description})
}

async fn list(app: &axum::Router, admin: &str, query: &str) -> Vec<Value> {
    let (status, v) = call(
        app,
        "GET",
        &format!("{ADMIN_LIST}{query}"),
        Some(admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    v.as_array().unwrap().clone()
}

// ---------------------------------------------------------------- 假 SMTP

/// 收到的一封信：信封收件人與 DATA 原文。
#[derive(Clone, Debug)]
struct Mail {
    recipients: Vec<String>,
    data: String,
}

impl Mail {
    fn header(&self, name: &str) -> Option<String> {
        let prefix = format!("{}:", name.to_ascii_lowercase());
        self.data
            .lines()
            .take_while(|l| !l.is_empty())
            .find(|l| l.to_ascii_lowercase().starts_with(&prefix))
            .map(|l| l[prefix.len()..].trim().to_string())
    }

    /// 解開 base64 內文（實作固定用 base64 傳輸編碼）。
    fn body(&self) -> String {
        let (_, body) = self.data.split_once("\r\n\r\n").expect("沒有內文");
        let joined: String = body.split_whitespace().collect();
        String::from_utf8(STANDARD.decode(joined).expect("內文不是 base64")).unwrap()
    }
}

struct FakeSmtp {
    port: u16,
    mails: Arc<Mutex<Vec<Mail>>>,
}

impl FakeSmtp {
    /// `reject_rcpt` 為真時，對收件人回 550（模擬被拒）。
    async fn start(reject_rcpt: bool) -> Self {
        Self::start_slow(reject_rcpt, Duration::ZERO).await
    }

    /// 與 `start` 相同，但收到信件內容後先等 `delay` 才回應（讓同時進來的重寄有機會重疊）。
    async fn start_slow(reject_rcpt: bool, delay: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let mails = Arc::new(Mutex::new(Vec::new()));
        let store = mails.clone();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let store = store.clone();
                tokio::spawn(async move {
                    let (r, mut w) = stream.into_split();
                    let mut r = BufReader::new(r);
                    let _ = w.write_all(b"220 fake ESMTP\r\n").await;
                    let mut recipients = Vec::new();
                    let mut line = String::new();
                    loop {
                        line.clear();
                        if r.read_line(&mut line).await.unwrap_or(0) == 0 {
                            return;
                        }
                        let cmd = line.trim_end().to_ascii_uppercase();
                        let reply: &[u8] = if cmd.starts_with("EHLO") {
                            b"250-fake\r\n250 8BITMIME\r\n"
                        } else if cmd.starts_with("RCPT TO") {
                            if reject_rcpt {
                                b"550 no such user\r\n"
                            } else {
                                recipients.push(line.trim_end()[8..].to_string());
                                b"250 ok\r\n"
                            }
                        } else if cmd == "DATA" {
                            let _ = w.write_all(b"354 go\r\n").await;
                            let mut data = String::new();
                            loop {
                                line.clear();
                                if r.read_line(&mut line).await.unwrap_or(0) == 0 {
                                    return;
                                }
                                if line == ".\r\n" {
                                    break;
                                }
                                data.push_str(&line);
                            }
                            tokio::time::sleep(delay).await;
                            store.lock().unwrap().push(Mail {
                                recipients: std::mem::take(&mut recipients),
                                data,
                            });
                            b"250 queued\r\n"
                        } else if cmd == "QUIT" {
                            let _ = w.write_all(b"221 bye\r\n").await;
                            return;
                        } else {
                            b"250 ok\r\n"
                        };
                        let _ = w.write_all(reply).await;
                    }
                });
            }
        });
        Self { port, mails }
    }

    fn config(&self) -> SmtpConfig {
        smtp_config(self.port)
    }

    fn mails(&self) -> Vec<Mail> {
        self.mails.lock().unwrap().clone()
    }
}

fn smtp_config(port: u16) -> SmtpConfig {
    SmtpConfig {
        host: "127.0.0.1".to_string(),
        port,
        username: None,
        password: None,
        security: Security::None,
        from: "Socrates <noreply@example.test>".to_string(),
        to: vec![
            "ops1@example.test".to_string(),
            "ops2@example.test".to_string(),
        ],
    }
}

/// 一個已經關掉的埠：連線會被拒。
async fn closed_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    l.local_addr().unwrap().port()
}

fn app_with_smtp(pool: PgPool, smtp: Option<SmtpConfig>) -> axum::Router {
    let mut config = test_config();
    config.smtp = smtp;
    test_app_with_config(pool, FakeAi::with(vec![]), config)
}

/// 等背景寄信結束（狀態不再是 pending）。
async fn settled(app: &axum::Router, admin: &str, id: &str) -> Value {
    for _ in 0..100 {
        let rows = list(app, admin, "").await;
        let row = rows
            .iter()
            .find(|r| r["id"] == id)
            .expect("客訴不在清單")
            .clone();
        if row["mail_status"] != "pending" {
            return row;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("寄信一直是 pending");
}

async fn only_complaint_id(app: &axum::Router, admin: &str) -> String {
    let rows = list(app, admin, "").await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    rows[0]["id"].as_str().unwrap().to_string()
}

// ---------------------------------------------------------------- 送出的權限

#[sqlx::test]
async fn only_logged_in_students_and_teachers_can_submit(pool: PgPool) {
    let app = app_with_smtp(pool, None);
    let a = seed_actors(&app).await;

    let (status, _) = call(&app, "POST", SUBMIT, None, Some(complaint("連不上"))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // 尚未開通（名單外）的帳號
    let (status, v) = submit(&app, &a.outsider, complaint("連不上")).await;
    assert_eq!(
        (status, v["error"]["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("not_enrolled"))
    );

    // 純管理者不是學生或教師
    let (status, v) = submit(&app, &a.admin, complaint("連不上")).await;
    assert_eq!(
        (status, v["error"]["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("forbidden"))
    );

    assert_eq!(
        submit(&app, &a.student, complaint("連不上")).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        submit(&app, &a.teacher, complaint("教師也能送")).await.0,
        StatusCode::NO_CONTENT
    );

    let rows = list(&app, &a.admin, "").await;
    assert_eq!(rows.len(), 2);
    let roles: Vec<_> = rows
        .iter()
        .map(|r| r["submitter_role"].as_str().unwrap())
        .collect();
    assert!(
        roles.contains(&"student") && roles.contains(&"teacher"),
        "{roles:?}"
    );
}

#[sqlx::test]
async fn description_and_category_are_validated(pool: PgPool) {
    let app = app_with_smtp(pool.clone(), None);
    let a = seed_actors(&app).await;

    for (body, code) in [
        (
            json!({"category": "connection", "description": "   "}),
            "invalid_description",
        ),
        (
            json!({"category": "connection", "description": ""}),
            "invalid_description",
        ),
        (complaint(&"字".repeat(2001)), "invalid_description"),
        // Postgres 的 text 欄位不能存 NUL；要在驗證時就回 400，而不是寫入失敗變 500
        (complaint("a\u{0}b"), "invalid_description"),
        (complaint("\u{0}"), "invalid_description"),
        (
            json!({"category": "nope", "description": "x"}),
            "invalid_category",
        ),
    ] {
        let (status, v) = submit(&app, &a.student, body).await;
        assert_eq!(
            (status, v["error"]["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some(code))
        );
    }
    assert!(list(&app, &a.admin, "").await.is_empty());

    // 上限正好 2000 字（以字元計，不是位元組），前後空白去掉
    let exactly = "字".repeat(2000);
    assert_eq!(
        submit(&app, &a.student, complaint(&format!("  {exactly}\n")))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let rows = list(&app, &a.admin, "").await;
    assert_eq!(rows[0]["description"], exactly);

    // 四種類別都收；每次送完清掉，避免撞到每小時 3 則的上限
    for category in ["connection", "voice", "content", "other"] {
        sqlx::query("DELETE FROM complaints")
            .execute(&pool)
            .await
            .unwrap();
        let body = json!({"category": category, "description": "x"});
        assert_eq!(
            submit(&app, &a.student, body).await.0,
            StatusCode::NO_CONTENT,
            "{category}"
        );
        assert_eq!(list(&app, &a.admin, "").await[0]["category"], category);
    }
}

#[sqlx::test]
async fn a_conversation_can_only_be_attached_by_its_owner(pool: PgPool) {
    let app = app_with_smtp(pool, None);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;

    let other = add_student(&app, &a, "sub-other", "other@example.com").await;

    let with = |id: &str| json!({"category": "content", "description": "這場對話有問題", "conversation_id": id});
    assert_eq!(
        submit(&app, &a.student, with(&conv)).await.0,
        StatusCode::NO_CONTENT
    );

    // 別人的對話、不存在的對話、教師附對話：一律 404，不透露是否存在
    for (cookie, id) in [
        (&other, conv.clone()),
        (&a.student, uuid::Uuid::new_v4().to_string()),
        (&a.teacher, conv.clone()),
    ] {
        let (status, v) = submit(&app, cookie, with(&id)).await;
        assert_eq!(
            (status, v["error"]["code"].as_str()),
            (StatusCode::NOT_FOUND, Some("not_found"))
        );
    }

    let rows = list(&app, &a.admin, "").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["conversation_id"], conv);
    assert_eq!(rows[0]["classroom_name"], "測試教室");
}

// ---------------------------------------------------------------- 限流

#[sqlx::test]
async fn at_most_three_per_hour_per_person(pool: PgPool) {
    let app = app_with_smtp(pool, None);
    let a = seed_actors(&app).await;
    let other = add_student(&app, &a, "sub-other", "other@example.com").await;

    for _ in 0..3 {
        assert_eq!(
            submit(&app, &a.student, complaint("x")).await.0,
            StatusCode::NO_CONTENT
        );
    }
    let (status, v) = submit(&app, &a.student, complaint("第四則")).await;
    assert_eq!(
        (status, v["error"]["code"].as_str()),
        (
            StatusCode::TOO_MANY_REQUESTS,
            Some("complaint_rate_limited")
        )
    );
    // 被擋下的不保存；別人不受影響
    assert_eq!(list(&app, &a.admin, "").await.len(), 3);
    assert_eq!(
        submit(&app, &other, complaint("別人")).await.0,
        StatusCode::NO_CONTENT
    );
}

async fn seed_old_complaints(pool: &PgPool, email: &str, count: i32, hours_ago: i32) {
    sqlx::query(
        "INSERT INTO complaints (user_id, category, description, submitter_role, created_at)
         SELECT id, 'other', 'seed', 'student', now() - make_interval(hours => $3, mins => g)
         FROM users, generate_series(1, $2) g WHERE email = $1",
    )
    .bind(email)
    .bind(count)
    .bind(hours_ago)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test]
async fn the_hourly_window_slides_and_the_daily_cap_is_ten(pool: PgPool) {
    let app = app_with_smtp(pool.clone(), None);
    let a = seed_actors(&app).await;

    // 3 則都在 2 小時前：不佔每小時額度，這一則可以送（今天共 4 則）
    seed_old_complaints(&pool, "student@example.com", 3, 2).await;
    assert_eq!(
        submit(&app, &a.student, complaint("現在的")).await.0,
        StatusCode::NO_CONTENT
    );

    // 再補 6 則在 5 小時前（今天共 10 則）：每小時額度還有，但每日已滿
    seed_old_complaints(&pool, "student@example.com", 6, 5).await;
    let (status, v) = submit(&app, &a.student, complaint("第十一則")).await;
    assert_eq!(
        (status, v["error"]["code"].as_str()),
        (
            StatusCode::TOO_MANY_REQUESTS,
            Some("complaint_rate_limited")
        )
    );

    // 超過 24 小時的不算進每日額度
    sqlx::query(
        "UPDATE complaints SET created_at = now() - interval '25 hours' WHERE description = 'seed'",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        submit(&app, &a.student, complaint("隔天")).await.0,
        StatusCode::NO_CONTENT
    );
}

// ---------------------------------------------------------------- 管理介面

#[sqlx::test]
async fn only_admins_can_list_view_and_change_complaints(pool: PgPool) {
    let app = app_with_smtp(pool, None);
    let a = seed_actors(&app).await;
    submit(&app, &a.student, complaint("私人的描述")).await;
    let id = only_complaint_id(&app, &a.admin).await;

    for (cookie, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.teacher.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            call(&app, "GET", ADMIN_LIST, cookie, None).await.0,
            expected
        );
        let patch = call(
            &app,
            "PATCH",
            &format!("{ADMIN_LIST}/{id}"),
            cookie,
            Some(json!({"status": "resolved"})),
        )
        .await;
        assert_eq!(patch.0, expected);
        assert_eq!(
            call(
                &app,
                "POST",
                &format!("{ADMIN_LIST}/{id}/resend"),
                cookie,
                None
            )
            .await
            .0,
            expected
        );
    }
    // 沒被改動
    assert_eq!(list(&app, &a.admin, "").await[0]["status"], "open");
}

#[sqlx::test]
async fn admin_sees_the_description_verbatim_with_the_submitter(pool: PgPool) {
    let app = app_with_smtp(pool, None);
    let a = seed_actors(&app).await;
    submit(
        &app,
        &a.student,
        json!({"category": "voice", "description": "語音一直斷線\n第二行"}),
    )
    .await;

    let row = list(&app, &a.admin, "").await.remove(0);
    assert_eq!(row["description"], "語音一直斷線\n第二行");
    assert_eq!(row["category"], "voice");
    assert_eq!(row["status"], "open");
    assert_eq!(row["submitter_name"], "學生");
    assert_eq!(row["submitter_role"], "student");
    assert!(row["conversation_id"].is_null());
    assert!(row["classroom_name"].is_null());
    // 不洩漏學號與電子郵件
    let text = row.to_string();
    assert!(
        !text.contains("10000000A") && !text.contains("student@example.com"),
        "{text}"
    );
}

#[sqlx::test]
async fn status_changes_are_audited_without_the_description(pool: PgPool) {
    let app = app_with_smtp(pool.clone(), None);
    let a = seed_actors(&app).await;
    submit(&app, &a.student, complaint("審計不能含這段文字")).await;
    let id = only_complaint_id(&app, &a.admin).await;
    let url = format!("{ADMIN_LIST}/{id}");

    let (status, v) = call(
        &app,
        "PATCH",
        &url,
        Some(&a.admin),
        Some(json!({"status": "resolved"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "resolved");
    // 沒有變更就不再記
    assert_eq!(
        call(
            &app,
            "PATCH",
            &url,
            Some(&a.admin),
            Some(json!({"status": "resolved"}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(list(&app, &a.admin, "?status=resolved").await.len(), 1);
    assert!(list(&app, &a.admin, "?status=open").await.is_empty());
    assert_eq!(
        call(
            &app,
            "PATCH",
            &url,
            Some(&a.admin),
            Some(json!({"status": "open"}))
        )
        .await
        .0,
        StatusCode::OK
    );

    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT action, target_email FROM audit_logs WHERE action LIKE 'complaint_%' ORDER BY created_at, id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![
            ("complaint_resolved".to_string(), Some(id.clone())),
            ("complaint_reopened".to_string(), Some(id.clone())),
        ]
    );

    let (status, v) = call(
        &app,
        "PATCH",
        &url,
        Some(&a.admin),
        Some(json!({"status": "bogus"})),
    )
    .await;
    assert_eq!(
        (status, v["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_status"))
    );
    let missing = format!("{ADMIN_LIST}/{}", uuid::Uuid::new_v4());
    assert_eq!(
        call(
            &app,
            "PATCH",
            &missing,
            Some(&a.admin),
            Some(json!({"status": "resolved"}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{missing}/resend"),
            Some(&a.admin),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

// ---------------------------------------------------------------- 寄信

#[sqlx::test]
async fn without_smtp_the_complaint_is_kept_and_the_mail_is_skipped(pool: PgPool) {
    let app = app_with_smtp(pool, None);
    let a = seed_actors(&app).await;
    assert_eq!(
        submit(&app, &a.student, complaint("沒有寄信設定")).await.0,
        StatusCode::NO_CONTENT
    );
    let id = only_complaint_id(&app, &a.admin).await;
    assert_eq!(settled(&app, &a.admin, &id).await["mail_status"], "skipped");
}

#[sqlx::test]
async fn the_mail_has_the_complaint_but_no_personal_identifiers(pool: PgPool) {
    let smtp = FakeSmtp::start(false).await;
    let app = app_with_smtp(pool.clone(), Some(smtp.config()));
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    seed_student_message(&pool, &conv, "對話原文不能出現在信裡").await;

    let body =
        json!({"category": "content", "description": "AI 的回答有問題", "conversation_id": conv});
    assert_eq!(
        submit(&app, &a.student, body).await.0,
        StatusCode::NO_CONTENT
    );
    let id = only_complaint_id(&app, &a.admin).await;
    assert_eq!(settled(&app, &a.admin, &id).await["mail_status"], "sent");

    let mails = smtp.mails();
    assert_eq!(mails.len(), 1);
    let mail = &mails[0];
    let mut to: Vec<_> = mail.recipients.clone();
    to.sort();
    assert_eq!(to, vec!["<ops1@example.test>", "<ops2@example.test>"]);
    assert!(
        mail.header("From")
            .unwrap()
            .contains("noreply@example.test")
    );
    // 標題只有編號與類別，不含描述
    assert_eq!(
        mail.header("Subject").unwrap(),
        format!("Complaint {id} (content)")
    );

    let text = mail.body();
    for expected in [
        id.as_str(),
        "content",
        "AI 的回答有問題",
        conv.as_str(),
        "學生",
        "student",
        "測試教室",
    ] {
        assert!(text.contains(expected), "信裡缺少 {expected}：\n{text}");
    }
    // 不含學號、電子郵件與對話原文
    for forbidden in ["10000000A", "student@example.com", "對話原文不能出現在信裡"] {
        assert!(
            !text.contains(forbidden),
            "信裡不該有 {forbidden}：\n{text}"
        );
        assert!(!mail.data.contains(forbidden), "信的標頭不該有 {forbidden}");
    }
}

#[sqlx::test]
async fn the_classroom_is_only_in_the_mail_when_a_conversation_is_attached(pool: PgPool) {
    let smtp = FakeSmtp::start(false).await;
    let app = app_with_smtp(pool, Some(smtp.config()));
    let a = seed_actors(&app).await;
    assert_eq!(
        submit(&app, &a.student, complaint("沒有附對話")).await.0,
        StatusCode::NO_CONTENT
    );
    let id = only_complaint_id(&app, &a.admin).await;
    settled(&app, &a.admin, &id).await;

    let text = smtp.mails()[0].body();
    assert!(
        text.contains("沒有附對話") && !text.contains("測試教室"),
        "{text}"
    );
}

#[sqlx::test]
async fn a_failed_mail_keeps_the_complaint_and_can_be_resent(pool: PgPool) {
    // 連線被拒：提交者照常成功，狀態 failed
    let app = app_with_smtp(pool.clone(), Some(smtp_config(closed_port().await)));
    let a = seed_actors(&app).await;
    assert_eq!(
        submit(&app, &a.student, complaint("寄不出去")).await.0,
        StatusCode::NO_CONTENT
    );
    let id = only_complaint_id(&app, &a.admin).await;
    assert_eq!(settled(&app, &a.admin, &id).await["mail_status"], "failed");

    // 管理者修好設定後重寄（同一個資料庫，換一個有伺服器的應用程式）
    let smtp = FakeSmtp::start(false).await;
    let fixed = app_with_smtp(pool.clone(), Some(smtp.config()));
    let admin = login_as(&fixed, "sub-admin", "admin@example.com").await;
    let (status, _) = call(
        &fixed,
        "POST",
        &format!("{ADMIN_LIST}/{id}/resend"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(settled(&fixed, &admin, &id).await["mail_status"], "sent");
    assert_eq!(smtp.mails().len(), 1);
}

#[sqlx::test]
async fn two_concurrent_resends_send_exactly_one_mail(pool: PgPool) {
    // 先在沒有 SMTP 的情況下送出，狀態是 skipped；之後有伺服器了，同時按兩次重寄
    let app = app_with_smtp(pool.clone(), None);
    let a = seed_actors(&app).await;
    assert_eq!(
        submit(&app, &a.student, complaint("連點重寄")).await.0,
        StatusCode::NO_CONTENT
    );
    let id = only_complaint_id(&app, &a.admin).await;
    assert_eq!(settled(&app, &a.admin, &id).await["mail_status"], "skipped");

    let smtp = FakeSmtp::start_slow(false, Duration::from_millis(500)).await;
    let fixed = app_with_smtp(pool.clone(), Some(smtp.config()));
    let admin = login_as(&fixed, "sub-admin", "admin@example.com").await;
    let path = format!("{ADMIN_LIST}/{id}/resend");
    let (first, second) = tokio::join!(
        call(&fixed, "POST", &path, Some(&admin), None),
        call(&fixed, "POST", &path, Some(&admin), None),
    );
    assert_eq!(first.0, StatusCode::NO_CONTENT);
    assert_eq!(second.0, StatusCode::NO_CONTENT);
    assert_eq!(settled(&fixed, &admin, &id).await["mail_status"], "sent");
    // 再等一下，確認沒有第二封晚到
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(smtp.mails().len(), 1);
}

#[sqlx::test]
async fn startup_requeues_only_stale_pending_complaints(pool: PgPool) {
    let smtp = FakeSmtp::start(false).await;
    let mut config = test_config();
    config.smtp = Some(smtp.config());
    register_fixture_pool(&pool);
    let state = AppState::new(
        pool.clone(),
        config,
        Arc::new(FakeIdentity),
        FakeAi::with(vec![]),
    );
    let app = socrates_chat_backend::app(state.clone());
    let a = seed_actors(&app).await;
    for text in ["卡住的", "剛開始寄的", "舊版留下的"] {
        // 直接寫入：不經過背景寄信
        sqlx::query(
            "INSERT INTO complaints (user_id, category, description, submitter_role)
             SELECT id, 'other', $1, 'student' FROM users WHERE email = 'student@example.com'",
        )
        .bind(text)
        .execute(&pool)
        .await
        .unwrap();
    }
    sqlx::query("UPDATE complaints SET mail_send_started_at = now() - interval '1 hour' WHERE description = '卡住的'")
        .execute(&pool).await.unwrap();
    sqlx::query(
        "UPDATE complaints SET mail_send_started_at = now() WHERE description = '剛開始寄的'",
    )
    .execute(&pool)
    .await
    .unwrap();
    // 「舊版留下的」沒有開始時間（migration 之前就卡住的列）
    let _ = &a;

    let n = socrates_chat_backend::complaints::requeue_stale(&state).await;
    assert_eq!(n, 2);
    let mut statuses = Vec::new();
    for _ in 0..100 {
        statuses = sqlx::query_as::<_, (String, String)>(
            "SELECT description, mail_status FROM complaints ORDER BY description",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        if statuses.iter().filter(|(_, s)| s == "sent").count() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    for (d, s) in &statuses {
        let expected = if d == "剛開始寄的" {
            "pending"
        } else {
            "sent"
        };
        assert_eq!(s, expected, "{d}");
    }
    assert_eq!(smtp.mails().len(), 2);
}

#[sqlx::test]
async fn a_rejecting_server_also_gives_failed(pool: PgPool) {
    let smtp = FakeSmtp::start(true).await;
    let app = app_with_smtp(pool, Some(smtp.config()));
    let a = seed_actors(&app).await;
    assert_eq!(
        submit(&app, &a.student, complaint("被拒")).await.0,
        StatusCode::NO_CONTENT
    );
    let id = only_complaint_id(&app, &a.admin).await;
    assert_eq!(settled(&app, &a.admin, &id).await["mail_status"], "failed");
}

// ---------------------------------------------------------------- 帳號刪除

#[sqlx::test]
async fn deleting_a_student_deletes_their_complaints_only(pool: PgPool) {
    let app = app_with_smtp(pool.clone(), None);
    let a = seed_actors(&app).await;
    let other = add_student(&app, &a, "sub-other", "other@example.com").await;
    submit(&app, &a.student, complaint("會被刪")).await;
    submit(&app, &other, complaint("留下來")).await;
    assert_eq!(list(&app, &a.admin, "").await.len(), 2);

    let (status, v) = call(
        &app,
        "DELETE",
        "/api/students/student@example.com",
        Some(&a.admin),
        Some(json!({"confirm_student_number": "10000000A"})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{v}");

    let rows = list(&app, &a.admin, "").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["description"], "留下來");
}
