//! F-06 對話與訊息：建立、送出、存取權限與刪除（合成資料）。

mod common;

use axum::{Router, http::StatusCode};
use common::*;
use serde_json::{Value, json};
use socrates_chat_backend::chat::MAX_MESSAGE_CHARS;
use sqlx::PgPool;

/// 教師建立並發布一個活動，回傳 id。
pub async fn published_activity(app: &Router, teacher: &str) -> String {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/activities",
            Some(teacher),
            Some(json!({"title": "電車難題", "description": "如果你是駕駛，你會怎麼做？"})),
        ),
    )
    .await;
    let id = json_body(res).await["id"].as_str().unwrap().to_string();
    send(
        app,
        json_req(
            "POST",
            &format!("/api/activities/{id}/publish"),
            Some(teacher),
            None,
        ),
    )
    .await;
    id
}

async fn start(app: &Router, cookie: &str, activity_id: &str) -> String {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/conversations",
            Some(cookie),
            Some(json!({"activity_id": activity_id})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    json_body(res).await["id"].as_str().unwrap().to_string()
}

async fn say(app: &Router, cookie: &str, conv: &str, text: &str) -> StatusCode {
    send(
        app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/messages"),
            Some(cookie),
            Some(json!({"content": text})),
        ),
    )
    .await
    .status()
}

#[sqlx::test]
async fn student_starts_conversation_from_published_activity(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let act = published_activity(&app, &a.teacher).await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/conversations",
            Some(&a.student),
            Some(json!({"activity_id": act})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let c = json_body(res).await;
    assert_eq!(c["title"], "電車難題");
    assert_eq!(c["status"], "active");
    assert_eq!(c["stage"], 1);
    assert_eq!(c["turn_count"], 0);
}

#[sqlx::test]
async fn student_starts_conversation_from_active_topic(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let topic = json_body(
        send(
            &app,
            json_req(
                "POST",
                "/api/admin/topics",
                Some(&a.admin),
                Some(json!({"title": "自由意志"})),
            ),
        )
        .await,
    )
    .await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/conversations",
            Some(&a.student),
            Some(json!({"topic_id": topic["id"]})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    assert_eq!(json_body(res).await["title"], "自由意志");

    // 停用後不能再用
    let id = topic["id"].as_str().unwrap();
    send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/admin/topics/{id}"),
            Some(&a.admin),
            Some(json!({"is_active": false})),
        ),
    )
    .await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/conversations",
            Some(&a.student),
            Some(json!({"topic_id": id})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn unavailable_sources_are_rejected(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    // 草稿活動
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({"title": "草稿"})),
        ),
    )
    .await;
    let draft = json_body(res).await["id"].clone();
    for body in [
        json!({"activity_id": draft}),
        json!({}),
        json!({"activity_id": uuid::Uuid::new_v4()}),
    ] {
        let res = send(
            &app,
            json_req("POST", "/api/conversations", Some(&a.student), Some(body)),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
}

#[sqlx::test]
async fn messages_are_saved_in_order_and_count_turns(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let act = published_activity(&app, &a.teacher).await;
    let conv = start(&app, &a.student, &act).await;

    assert_eq!(
        say(&app, &a.student, &conv, "我會轉向。").await,
        StatusCode::CREATED
    );
    assert_eq!(
        say(&app, &a.student, &conv, "因為五個人比一個人重要。").await,
        StatusCode::CREATED
    );

    let d = json_body(
        send(
            &app,
            get(&format!("/api/conversations/{conv}"), Some(&a.student)),
        )
        .await,
    )
    .await;
    assert_eq!(d["turn_count"], 2);
    let msgs = d["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0]["content"], "我會轉向。");
    assert_eq!(msgs[1]["content"], "因為五個人比一個人重要。");
    assert_eq!(msgs[0]["role"], "student");
    assert_eq!(msgs[0]["source"], "text");

    let list = json_body(send(&app, get("/api/conversations", Some(&a.student))).await).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    // 列表不含訊息原文
    assert!(list[0].get("messages").is_none());
}

#[sqlx::test]
async fn message_validation(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let act = published_activity(&app, &a.teacher).await;
    let conv = start(&app, &a.student, &act).await;

    assert_eq!(
        say(&app, &a.student, &conv, "   ").await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        say(&app, &a.student, &conv, &"字".repeat(MAX_MESSAGE_CHARS + 1)).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        say(&app, &a.student, &conv, &"字".repeat(MAX_MESSAGE_CHARS)).await,
        StatusCode::CREATED
    );
    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/messages"),
            Some(&a.student),
            Some(json!({"content": "你好", "source": "speech-browser"})),
        ),
    )
    .await;
    assert_eq!(json_body(res).await["source"], "speech-browser");
    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/messages"),
            Some(&a.student),
            Some(json!({"content": "你好", "source": "carrier-pigeon"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn ended_conversation_rejects_new_messages(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let act = published_activity(&app, &a.teacher).await;
    let conv = start(&app, &a.student, &act).await;
    sqlx::query("UPDATE conversations SET status = 'ended'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        say(&app, &a.student, &conv, "還能說嗎").await,
        StatusCode::CONFLICT
    );
}

/// 授權：他人、教師、管理者、名單外與未登入都碰不到別人的對話內容。
#[sqlx::test]
async fn conversation_content_is_owner_only(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    send(
        &app,
        json_req(
            "POST",
            "/api/roster/import",
            Some(&a.admin),
            Some(json!({"text": "other@example.com"})),
        ),
    )
    .await;
    let other = login_as(&app, "sub-other", "other@example.com").await;
    let act = published_activity(&app, &a.teacher).await;
    let conv = start(&app, &a.student, &act).await;
    say(&app, &a.student, &conv, "這是我的私人想法").await;

    for cookie in [&other] {
        // 讀取、送訊息、刪除都是 404（不洩漏存在與否）
        let res = send(
            &app,
            get(&format!("/api/conversations/{conv}"), Some(cookie)),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            say(&app, cookie, &conv, "偷說話").await,
            StatusCode::NOT_FOUND
        );
        let res = send(
            &app,
            json_req(
                "DELETE",
                &format!("/api/conversations/{conv}"),
                Some(cookie),
                None,
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        // 別人的列表是空的，且回應中沒有原文
        let list: Value =
            json_body(send(&app, get("/api/conversations", Some(cookie))).await).await;
        assert!(!list.to_string().contains("私人想法"));
    }

    // 教師與管理者不參與討論：學生功能一律 403 forbidden（連別人的對話是否存在都不碰）
    for cookie in [&a.teacher, &a.admin] {
        let res = send(
            &app,
            get(&format!("/api/conversations/{conv}"), Some(cookie)),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(json_body(res).await["error"]["code"], "forbidden");
        assert_eq!(
            say(&app, cookie, &conv, "偷說話").await,
            StatusCode::FORBIDDEN
        );
        let res = send(
            &app,
            json_req(
                "POST",
                "/api/conversations",
                Some(cookie),
                Some(json!({"activity_id": act})),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    // 名單外與未登入
    let res = send(
        &app,
        get(&format!("/api/conversations/{conv}"), Some(&a.outsider)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(res).await["error"]["code"], "not_enrolled");
    let res = send(&app, get(&format!("/api/conversations/{conv}"), None)).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        say(&app, &a.outsider, &conv, "x").await,
        StatusCode::FORBIDDEN
    );
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/conversations",
            Some(&a.outsider),
            Some(json!({"activity_id": act})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 擁有者仍然讀得到
    let res = send(
        &app,
        get(&format!("/api/conversations/{conv}"), Some(&a.student)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[sqlx::test]
async fn owner_deletes_conversation_with_messages(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let act = published_activity(&app, &a.teacher).await;
    let conv = start(&app, &a.student, &act).await;
    say(&app, &a.student, &conv, "hello").await;

    let res = send(
        &app,
        json_req(
            "DELETE",
            &format!("/api/conversations/{conv}"),
            Some(&a.student),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM messages")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    let res = send(
        &app,
        get(&format!("/api/conversations/{conv}"), Some(&a.student)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn conversation_keeps_snapshot_when_activity_changes(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let act = published_activity(&app, &a.teacher).await;
    let conv = start(&app, &a.student, &act).await;
    send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/activities/{act}"),
            Some(&a.teacher),
            Some(json!({"title": "改名了"})),
        ),
    )
    .await;
    let d = json_body(
        send(
            &app,
            get(&format!("/api/conversations/{conv}"), Some(&a.student)),
        )
        .await,
    )
    .await;
    assert_eq!(d["title"], "電車難題");
}
