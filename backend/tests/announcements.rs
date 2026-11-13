//! F-34 / S-15：公告與維護模式。全部使用合成資料。

mod common;

use std::time::Duration;

use axum::{body::Body, http::Request, http::StatusCode};
use chrono::{DateTime, Utc};
use common::Script::*;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

const ADMIN_LIST: &str = "/api/admin/announcements";
const PUBLIC: &str = "/api/announcements";
const MAINTENANCE: &str = "/api/admin/maintenance";

fn at(offset_secs: i64) -> String {
    let t: DateTime<Utc> = Utc::now() + chrono::Duration::seconds(offset_secs);
    t.to_rfc3339()
}

async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let res = send(app, json_req(method, path, cookie, body)).await;
    let status = res.status();
    let bytes = if status == StatusCode::NO_CONTENT {
        Value::Null
    } else {
        json_body(res).await
    };
    (status, bytes)
}

/// 建立一則公告並回傳它；AI 腳本決定翻譯成功或失敗。
async fn create(app: &axum::Router, admin: &str, body: Value) -> Value {
    let (status, v) = call(app, "POST", ADMIN_LIST, Some(admin), Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    v
}

fn translated() -> Vec<common::Script> {
    vec![Reply(vec!["Hello"]), Reply(vec!["Hola"])]
}

async fn public(app: &axum::Router, lang: Option<&str>) -> Value {
    let path = match lang {
        Some(l) => format!("{PUBLIC}?lang={l}"),
        None => PUBLIC.to_string(),
    };
    let res = send(app, get(&path, None)).await;
    assert_eq!(res.status(), StatusCode::OK);
    json_body(res).await
}

async fn set_maintenance(app: &axum::Router, admin: &str, on: bool) {
    let (status, v) = call(
        app,
        "PUT",
        MAINTENANCE,
        Some(admin),
        Some(json!({"enabled": on})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["enabled"], on);
}

async fn audit_actions(pool: &PgPool, prefix: &str) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT action, target_email FROM audit_logs WHERE action LIKE $1 ORDER BY created_at, id",
    )
    .bind(format!("{prefix}%"))
    .fetch_all(pool)
    .await
    .unwrap()
}

// ---------- 公告：授權 ----------

#[sqlx::test]
async fn only_admins_can_manage_announcements(pool: PgPool) {
    let app = test_app_with_ai(pool, FakeAi::with(vec![]));
    let a = seed_actors(&app).await;
    let body = json!({"kind": "general", "text_zh_tw": "不該建立"});

    let (status, _) = call(&app, "POST", ADMIN_LIST, None, Some(body.clone())).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    for who in [&a.student, &a.teacher, &a.outsider] {
        let (status, _) = call(&app, "POST", ADMIN_LIST, Some(who), Some(body.clone())).await;
        assert!(
            status == StatusCode::FORBIDDEN,
            "role should be refused: {status}"
        );
        let (status, _) = call(&app, "GET", ADMIN_LIST, Some(who), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    // 沒有任何一則被建立
    let (_, list) = call(&app, "GET", ADMIN_LIST, Some(&a.admin), None).await;
    assert_eq!(list.as_array().unwrap().len(), 0);
}

#[sqlx::test]
async fn only_admins_can_edit_or_delete_announcements(pool: PgPool) {
    let app = test_app_with_ai(pool, FakeAi::with(translated()));
    let a = seed_actors(&app).await;
    let made = create(
        &app,
        &a.admin,
        json!({"kind": "general", "text_zh_tw": "原內文"}),
    )
    .await;
    let path = format!("{ADMIN_LIST}/{}", made["id"].as_str().unwrap());
    for who in [&a.student, &a.teacher] {
        let (s, _) = call(
            &app,
            "PATCH",
            &path,
            Some(who),
            Some(json!({"enabled": false})),
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, _) = call(&app, "DELETE", &path, Some(who), None).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
    }
    let (s, _) = call(&app, "DELETE", &path, None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

// ---------- 公告：有效條件與免登入端點 ----------

#[sqlx::test]
async fn the_public_endpoint_needs_no_login_and_lists_only_active_announcements(pool: PgPool) {
    let ai = FakeAi::with(
        (0..10)
            .flat_map(|_| [Reply(vec!["x"]), Reply(vec!["y"])])
            .collect(),
    );
    let app = test_app_with_ai(pool, ai);
    let a = seed_actors(&app).await;
    create(
        &app,
        &a.admin,
        json!({"kind": "general", "text_zh_tw": "進行中"}),
    )
    .await;
    create(
        &app,
        &a.admin,
        json!({"kind": "general", "text_zh_tw": "已停用", "enabled": false}),
    )
    .await;
    create(
        &app,
        &a.admin,
        json!({"kind": "downtime", "text_zh_tw": "還沒開始", "starts_at": at(3600)}),
    )
    .await;
    create(
        &app,
        &a.admin,
        json!({"kind": "general", "text_zh_tw": "已結束", "starts_at": at(-7200), "ends_at": at(-3600)}),
    )
    .await;
    create(
        &app,
        &a.admin,
        json!({"kind": "downtime", "text_zh_tw": "後來的公告", "ends_at": at(3600)}),
    )
    .await;

    let v = public(&app, None).await;
    assert_eq!(v["maintenance"], false);
    let texts: Vec<&str> = v["announcements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["text"].as_str().unwrap())
        .collect();
    // 新的在前；停用、未開始、已結束的都不出現
    assert_eq!(texts, ["後來的公告", "進行中"]);
    let first = &v["announcements"][0];
    assert_eq!(first["kind"], "downtime");
    // 公開端點不帶稽核資訊或管理欄位
    assert!(first.get("enabled").is_none());
    assert!(first.get("pending_languages").is_none());
    assert!(first.get("created_by").is_none());
}

#[sqlx::test]
async fn the_public_endpoint_picks_the_language_and_falls_back_to_zh_tw(pool: PgPool) {
    let app = test_app_with_ai(pool, FakeAi::with(translated()));
    let a = seed_actors(&app).await;
    create(
        &app,
        &a.admin,
        json!({"kind": "general", "text_zh_tw": "明天維護"}),
    )
    .await;

    let text = |v: &Value| v["announcements"][0]["text"].as_str().unwrap().to_string();
    assert_eq!(text(&public(&app, Some("en")).await), "Hello");
    assert_eq!(text(&public(&app, Some("es")).await), "Hola");
    assert_eq!(text(&public(&app, Some("zh-TW")).await), "明天維護");
    // 沒指定或不支援的語言一律 zh-TW
    assert_eq!(text(&public(&app, None).await), "明天維護");
    assert_eq!(text(&public(&app, Some("fr")).await), "明天維護");
}

// ---------- 公告：翻譯 ----------

#[sqlx::test]
async fn a_failed_translation_keeps_the_announcement_and_marks_it_pending(pool: PgPool) {
    // 兩次翻譯都失敗（腳本用完一律失敗）
    let app = test_app_with_ai(pool, FakeAi::with(vec![Fail, Fail]));
    let a = seed_actors(&app).await;
    let made = create(
        &app,
        &a.admin,
        json!({"kind": "general", "text_zh_tw": "照常生效"}),
    )
    .await;

    assert_eq!(made["pending_languages"], json!(["en", "es"]));
    assert!(made["text_en"].is_null() && made["text_es"].is_null());
    // en 使用者先看到 zh-TW
    let v = public(&app, Some("en")).await;
    assert_eq!(v["announcements"][0]["text"], "照常生效");
}

#[sqlx::test]
async fn editing_zh_tw_translates_again_and_overwrites_manual_translations(pool: PgPool) {
    let ai = FakeAi::with(vec![
        Reply(vec!["Hello"]),
        Reply(vec!["Hola"]),
        Reply(vec!["Bye"]),
        Reply(vec!["Adiós"]),
    ]);
    let app = test_app_with_ai(pool, ai);
    let a = seed_actors(&app).await;
    let made = create(
        &app,
        &a.admin,
        json!({"kind": "general", "text_zh_tw": "你好"}),
    )
    .await;
    let path = format!("{ADMIN_LIST}/{}", made["id"].as_str().unwrap());

    // 管理者手動改譯文
    let (s, v) = call(
        &app,
        "PATCH",
        &path,
        Some(&a.admin),
        Some(json!({"text_en": "Hi there"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["text_en"], "Hi there");
    assert_eq!(v["text_es"], "Hola");

    // 再改 zh-TW：重新翻譯並覆蓋
    let (s, v) = call(
        &app,
        "PATCH",
        &path,
        Some(&a.admin),
        Some(json!({"text_zh_tw": "再見"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["text_zh_tw"], "再見");
    assert_eq!(v["text_en"], "Bye");
    assert_eq!(v["text_es"], "Adiós");
}

#[sqlx::test]
async fn pending_translations_can_be_retried_and_then_clear(pool: PgPool) {
    let ai = FakeAi::with(vec![Fail, Fail, Reply(vec!["Hello"]), Reply(vec!["Hola"])]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let a = seed_actors(&app).await;
    let made = create(
        &app,
        &a.admin,
        json!({"kind": "general", "text_zh_tw": "稍後翻譯"}),
    )
    .await;
    let id: uuid::Uuid = made["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(made["pending_languages"], json!(["en", "es"]));

    // 系統自動重試呼叫的就是這個函式
    let state = socrates_chat_backend::AppState::new(
        pool.clone(),
        test_config(),
        std::sync::Arc::new(FakeIdentity),
        ai,
    );
    socrates_chat_backend::announcements::retry_pending_translation(&state, id).await;

    let (_, list) = call(&app, "GET", ADMIN_LIST, Some(&a.admin), None).await;
    assert_eq!(list[0]["pending_languages"], json!([]));
    assert_eq!(list[0]["text_en"], "Hello");
    assert_eq!(list[0]["text_es"], "Hola");
}

// ---------- 公告：驗證、修改、刪除、稽核 ----------

#[sqlx::test]
async fn invalid_announcements_are_rejected(pool: PgPool) {
    let app = test_app_with_ai(pool, FakeAi::with(vec![]));
    let a = seed_actors(&app).await;
    let cases = [
        (
            json!({"kind": "general", "text_zh_tw": "   "}),
            "invalid_text",
        ),
        (
            json!({"kind": "general", "text_zh_tw": "字".repeat(1001)}),
            "invalid_text",
        ),
        (json!({"kind": "alert", "text_zh_tw": "x"}), "invalid_kind"),
        (
            json!({"kind": "general", "text_zh_tw": "x", "starts_at": at(100), "ends_at": at(50)}),
            "invalid_period",
        ),
    ];
    for (body, code) in cases {
        let res = send(
            &app,
            json_req("POST", ADMIN_LIST, Some(&a.admin), Some(body)),
        )
        .await;
        assert_json_error(res, StatusCode::BAD_REQUEST, code).await;
    }
}

#[sqlx::test]
async fn nul_characters_in_announcement_text_are_rejected_without_writing(pool: PgPool) {
    // Postgres 的 text 欄位不能存 NUL；要在驗證時就回 400，而不是寫入失敗變 500
    let app = test_app_with_ai(pool.clone(), FakeAi::with(translated()));
    let a = seed_actors(&app).await;
    let made = create(
        &app,
        &a.admin,
        json!({"kind": "general", "text_zh_tw": "原文"}),
    )
    .await;
    let path = format!("{ADMIN_LIST}/{}", made["id"].as_str().unwrap());
    let (_, before) = call(&app, "GET", ADMIN_LIST, Some(&a.admin), None).await;
    let audit_before = audit_actions(&pool, "announcement_").await;

    for body in [
        json!({"kind": "general", "text_zh_tw": "a\u{0}b"}),
        json!({"kind": "general", "text_zh_tw": "\u{0}"}),
    ] {
        let (s, v) = call(&app, "POST", ADMIN_LIST, Some(&a.admin), Some(body)).await;
        assert_eq!(
            (s, v["error"]["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some("invalid_text")),
            "{v}"
        );
    }
    for body in [
        json!({"text_zh_tw": "改\u{0}了"}),
        json!({"text_zh_tw": "\u{0}"}),
        json!({"text_en": "a\u{0}b"}),
        json!({"text_es": "\u{0}"}),
    ] {
        let (s, v) = call(&app, "PATCH", &path, Some(&a.admin), Some(body)).await;
        assert_eq!(
            (s, v["error"]["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some("invalid_text")),
            "{v}"
        );
    }

    // 什麼都沒寫：公告內容與稽核日誌都和之前一樣
    let (_, after) = call(&app, "GET", ADMIN_LIST, Some(&a.admin), None).await;
    assert_eq!(after, before);
    assert_eq!(audit_actions(&pool, "announcement_").await, audit_before);
}

#[sqlx::test]
async fn announcements_can_be_disabled_rescheduled_cleared_and_deleted(pool: PgPool) {
    let app = test_app_with_ai(pool, FakeAi::with(translated()));
    let a = seed_actors(&app).await;
    let made = create(
        &app,
        &a.admin,
        json!({"kind": "general", "text_zh_tw": "會被管理", "ends_at": at(3600)}),
    )
    .await;
    let path = format!("{ADMIN_LIST}/{}", made["id"].as_str().unwrap());

    // null 清除結束時間
    let (s, v) = call(
        &app,
        "PATCH",
        &path,
        Some(&a.admin),
        Some(json!({"ends_at": null})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["ends_at"].is_null());

    // 停用後公開端點看不到
    let (s, _) = call(
        &app,
        "PATCH",
        &path,
        Some(&a.admin),
        Some(json!({"enabled": false})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(public(&app, None).await["announcements"], json!([]));

    let (s, _) = call(&app, "DELETE", &path, Some(&a.admin), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, list) = call(&app, "GET", ADMIN_LIST, Some(&a.admin), None).await;
    assert_eq!(list, json!([]));
    let (s, _) = call(&app, "DELETE", &path, Some(&a.admin), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn announcement_changes_are_audited_without_the_text(pool: PgPool) {
    let app = test_app_with_ai(pool.clone(), FakeAi::with(translated()));
    let a = seed_actors(&app).await;
    let made = create(
        &app,
        &a.admin,
        json!({"kind": "general", "text_zh_tw": "機密不外洩文字"}),
    )
    .await;
    let id = made["id"].as_str().unwrap().to_string();
    let path = format!("{ADMIN_LIST}/{id}");
    call(
        &app,
        "PATCH",
        &path,
        Some(&a.admin),
        Some(json!({"enabled": false})),
    )
    .await;
    call(&app, "DELETE", &path, Some(&a.admin), None).await;

    let rows = audit_actions(&pool, "announcement_").await;
    assert_eq!(
        rows,
        [
            ("announcement_created".to_string(), id.clone()),
            ("announcement_updated".to_string(), id.clone()),
            ("announcement_deleted".to_string(), id.clone()),
        ]
    );
    // 稽核日誌不記內文（包含舊值與新值欄位）
    let leaked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE concat_ws(' ', target_email, reason, old_value::text, new_value::text) LIKE '%機密不外洩文字%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(leaked, 0);
}

// ---------- 維護模式：開關與授權 ----------

#[sqlx::test]
async fn only_admins_can_switch_maintenance_mode(pool: PgPool) {
    let app = test_app_with_ai(pool, FakeAi::with(vec![]));
    let a = seed_actors(&app).await;
    let body = json!({"enabled": true});
    let (s, _) = call(&app, "PUT", MAINTENANCE, None, Some(body.clone())).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    for who in [&a.student, &a.teacher, &a.outsider] {
        let (s, _) = call(&app, "PUT", MAINTENANCE, Some(who), Some(body.clone())).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, _) = call(&app, "GET", MAINTENANCE, Some(who), None).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
    }
    let (s, v) = call(&app, "GET", MAINTENANCE, Some(&a.admin), None).await;
    assert_eq!((s, &v["enabled"]), (StatusCode::OK, &json!(false)));
}

#[sqlx::test]
async fn switching_maintenance_mode_is_audited(pool: PgPool) {
    let app = test_app_with_ai(pool.clone(), FakeAi::with(vec![]));
    let a = seed_actors(&app).await;
    set_maintenance(&app, &a.admin, true).await;
    set_maintenance(&app, &a.admin, false).await;
    let rows = audit_actions(&pool, "maintenance_").await;
    let actions: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(actions, ["maintenance_enabled", "maintenance_disabled"]);
}

// ---------- 維護模式：行為 ----------

#[sqlx::test]
async fn maintenance_blocks_everyone_but_admins_and_the_exempt_endpoints(pool: PgPool) {
    let app = test_app_with_ai(pool, FakeAi::with(vec![]));
    let a = seed_actors(&app).await;
    set_maintenance(&app, &a.admin, true).await;

    // 非管理者與未登入：503 maintenance
    for cookie in [
        Some(a.student.as_str()),
        Some(a.teacher.as_str()),
        Some(a.outsider.as_str()),
        None,
    ] {
        let res = send(&app, get("/api/me", cookie)).await;
        assert_json_error(res, StatusCode::SERVICE_UNAVAILABLE, "maintenance").await;
    }
    // 管理者照常
    let res = send(&app, get("/api/me", Some(&a.admin))).await;
    assert_eq!(res.status(), StatusCode::OK);
    // 例外：健康檢查、就緒檢查、公告端點
    for path in ["/health", "/ready", PUBLIC] {
        let res = send(&app, get(path, None)).await;
        assert_eq!(res.status(), StatusCode::OK, "{path}");
    }
    assert_eq!(public(&app, None).await["maintenance"], true);
    // 登出照常（非管理者也能登出）
    let res = send(
        &app,
        json_req("POST", "/api/auth/logout", Some(&a.student), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    // 關閉後恢復
    set_maintenance(&app, &a.admin, false).await;
    let res = send(&app, get("/api/me", Some(&a.teacher))).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(public(&app, None).await["maintenance"], false);
}

#[sqlx::test]
async fn maintenance_survives_a_restart(pool: PgPool) {
    let app = test_app_with_ai(pool.clone(), FakeAi::with(vec![]));
    let a = seed_actors(&app).await;
    set_maintenance(&app, &a.admin, true).await;

    // 全新的 AppState（模擬後端重啟）：旗標從資料庫讀回
    let restarted = test_app_with_ai(pool, FakeAi::with(vec![]));
    let res = send(&restarted, get("/api/me", Some(&a.student))).await;
    assert_json_error(res, StatusCode::SERVICE_UNAVAILABLE, "maintenance").await;
    assert_eq!(public(&restarted, None).await["maintenance"], true);
}

#[sqlx::test]
async fn non_admins_cannot_log_in_during_maintenance(pool: PgPool) {
    let app = test_app_with_ai(pool.clone(), FakeAi::with(vec![]));
    let a = seed_actors(&app).await;
    // 先為學生建立臨時密碼，之後才開維護模式
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/users/reset-password",
            Some(&a.admin),
            Some(json!({"email": "student@example.com"})),
        ),
    )
    .await;
    let temp = json_body(res).await["temporary_password"]
        .as_str()
        .unwrap()
        .to_string();
    set_maintenance(&app, &a.admin, true).await;
    let sessions_before: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();

    // 密碼登入：503 maintenance，不建立 session
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/auth/login",
            None,
            Some(json!({"account": "student@example.com", "password": temp})),
        ),
    )
    .await;
    assert!(session_cookie(&res).is_none());
    assert_json_error(res, StatusCode::SERVICE_UNAVAILABLE, "maintenance").await;

    // Google 登入：不核發 session
    let res = send(
        &app,
        Request::get("/api/auth/google/login")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let state = location(&res).split("state=").nth(1).unwrap().to_string();
    let res = send(
        &app,
        Request::get(format!(
            "/api/auth/google/callback?code=ok:sub-student:student@example.com&state={state}"
        ))
        .body(Body::empty())
        .unwrap(),
    )
    .await;
    assert!(session_cookie(&res).is_none());

    let sessions_after: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sessions_before, sessions_after);

    // 管理者仍能登入
    let admin = login_as(&app, "sub-admin", "admin@example.com").await;
    let res = send(&app, get("/api/me", Some(&admin))).await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[sqlx::test]
async fn a_reply_in_progress_finishes_but_new_messages_are_blocked(pool: PgPool) {
    let pace = Duration::from_millis(150);
    let ai = FakeAi::with(vec![
        Paced(vec!["你為什麼", "這樣認為？"], pace),
        Reply(vec!["x"]),
    ]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;

    // 送出訊息：標頭一回來串流就在進行
    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/messages"),
            Some(&a.student),
            Some(json!({"content": "我認為應該拉桿"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    set_maintenance(&app, &a.admin, true).await;

    // 進行中的回覆讓它完成並保存
    let events = parse_sse(&text_body(res).await);
    assert!(events.iter().any(|(n, _)| n == "done"), "{events:?}");
    let saved: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM messages WHERE conversation_id = $1 AND role = 'ai'",
    )
    .bind(conv.parse::<uuid::Uuid>().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(saved, 1);

    // 新的訊息被擋
    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/messages"),
            Some(&a.student),
            Some(json!({"content": "下一句"})),
        ),
    )
    .await;
    assert_json_error(res, StatusCode::SERVICE_UNAVAILABLE, "maintenance").await;
}

#[sqlx::test]
async fn maintenance_also_blocks_the_internal_cascade_endpoint(pool: PgPool) {
    let app = test_app_with_ai(pool, FakeAi::with(vec![]));
    let a = seed_actors(&app).await;
    set_maintenance(&app, &a.admin, true).await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/internal/cascade/v1/chat/completions",
            None,
            Some(json!({"messages": []})),
        ),
    )
    .await;
    assert_json_error(res, StatusCode::SERVICE_UNAVAILABLE, "maintenance").await;
}
