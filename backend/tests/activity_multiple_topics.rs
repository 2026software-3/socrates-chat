//! F-29.1 活動可含多個題目：資料模型與活動 API（S-04.7、Issue #386）。
//! 涵蓋：多題目保存與順序、空清單 400、重複題目 400、不存在題目 400、
//! 鎖定前可自由增刪更換重排、已有對話後移除/更換/重排回 409、已有對話後新增題目成功。

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test]
async fn create_activity_with_multiple_topics_preserves_order(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let t1 = create_topic(&app, &a, "電車難題").await;
    let t2 = create_topic(&app, &a, "說謊可以嗎").await;
    let t3 = create_topic(&app, &a, "自由意志存在嗎").await;

    // 建立含多個題目的活動
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({
                "title": "倫理學討論",
                "classroom_id": a.classroom,
                "topic_ids": [&t2, &t1, &t3]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let body = json_body(res).await;
    let id = body["id"].as_str().unwrap();

    let expected_order = json!([&t2, &t1, &t3]);
    assert_eq!(body["topic_ids"], expected_order);

    // GET /api/activities/{id} 保留順序
    let res = send(
        &app,
        get(&format!("/api/activities/{id}"), Some(&a.teacher)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let got = json_body(res).await;
    assert_eq!(got["topic_ids"], expected_order);

    // GET /api/activities 列表也帶出 topic_ids
    let res = send(&app, get("/api/activities", Some(&a.teacher))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let list = json_body(res).await;
    let act = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == id)
        .unwrap();
    assert_eq!(act["topic_ids"], expected_order);
}

#[sqlx::test]
async fn create_activity_validates_topic_ids(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let t1 = create_topic(&app, &a, "電車難題").await;
    let t2 = create_topic(&app, &a, "說謊可以嗎").await;

    // 空清單回 400 invalid_topic
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({
                "title": "無題目活動",
                "classroom_id": a.classroom,
                "topic_ids": []
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(res).await["error"]["code"], "invalid_topic");

    // 缺少 topic_ids 回 400 invalid_topic
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({
                "title": "缺少題目活動",
                "classroom_id": a.classroom
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(res).await["error"]["code"], "invalid_topic");

    // 重複題目回 400 invalid_topic
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({
                "title": "重複題目活動",
                "classroom_id": a.classroom,
                "topic_ids": [&t1, &t2, &t1]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(res).await["error"]["code"], "invalid_topic");

    // 不存在的題目回 400 invalid_topic
    let ghost = Uuid::new_v4().to_string();
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({
                "title": "幽靈題目活動",
                "classroom_id": a.classroom,
                "topic_ids": [&t1, &ghost]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(res).await["error"]["code"], "invalid_topic");

    // 超過 5 個題目回 400 invalid_topic（S-04.7 補充決定：最多 5 個題目）
    let t3 = create_topic(&app, &a, "自由意志存在嗎").await;
    let t4 = create_topic(&app, &a, "道德相對主義").await;
    let t5 = create_topic(&app, &a, "心靈與身體").await;
    let t6 = create_topic(&app, &a, "認識論難題").await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({
                "title": "超過五題活動",
                "classroom_id": a.classroom,
                "topic_ids": [&t1, &t2, &t3, &t4, &t5, &t6]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(res).await["error"]["code"], "invalid_topic");
}

#[sqlx::test]
async fn patch_activity_topics_before_conversations(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let t1 = create_topic(&app, &a, "電車難題").await;
    let t2 = create_topic(&app, &a, "說謊可以嗎").await;
    let t3 = create_topic(&app, &a, "自由意志存在嗎").await;

    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({
                "title": "活動",
                "classroom_id": a.classroom,
                "topic_ids": [&t1, &t2]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let id = json_body(res).await["id"].as_str().unwrap().to_string();
    let path = format!("/api/activities/{id}");

    let patch = |body: Value| {
        let app = app.clone();
        let path = path.clone();
        let cookie = a.teacher.clone();
        async move { send(&app, json_req("PATCH", &path, Some(&cookie), Some(body))).await }
    };

    // 重排題目：[t2, t1]
    let res = patch(json!({"topic_ids": [&t2, &t1]})).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_body(res).await["topic_ids"], json!([&t2, &t1]));

    // 更換與增刪：換成 [t2, t3]
    let res = patch(json!({"topic_ids": [&t2, &t3]})).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_body(res).await["topic_ids"], json!([&t2, &t3]));

    // 刪減至只剩一個題目：[t3]
    let res = patch(json!({"topic_ids": [&t3]})).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_body(res).await["topic_ids"], json!([&t3]));

    // 驗證 PATCH 不合法情況：空清單、null、重複、超過 5 題
    let res = patch(json!({"topic_ids": []})).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(res).await["error"]["code"], "invalid_topic");

    let res = patch(json!({"topic_ids": null})).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(res).await["error"]["code"], "invalid_topic");

    let res = patch(json!({"topic_ids": [&t3, &t3]})).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(res).await["error"]["code"], "invalid_topic");

    let t4 = create_topic(&app, &a, "道德相對主義").await;
    let t5 = create_topic(&app, &a, "心靈與身體").await;
    let t6 = create_topic(&app, &a, "認識論難題").await;
    let res = patch(json!({"topic_ids": [&t1, &t2, &t3, &t4, &t5, &t6]})).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(res).await["error"]["code"], "invalid_topic");
}

#[sqlx::test]
async fn patch_activity_topics_locked_after_conversations(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let t1 = create_topic(&app, &a, "電車難題").await;
    let t2 = create_topic(&app, &a, "說謊可以嗎").await;
    let t3 = create_topic(&app, &a, "自由意志存在嗎").await;
    let t4 = create_topic(&app, &a, "道德相對主義").await;

    // 建立包含 t1, t2 的活動並發布
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({
                "title": "第一週討論",
                "classroom_id": a.classroom,
                "topic_ids": [&t1, &t2]
            })),
        ),
    )
    .await;
    let id = json_body(res).await["id"].as_str().unwrap().to_string();
    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/activities/{id}/publish"),
            Some(&a.teacher),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    let path = format!("/api/activities/{id}");
    let patch = |body: Value| {
        let app = app.clone();
        let path = path.clone();
        let cookie = a.teacher.clone();
        async move { send(&app, json_req("PATCH", &path, Some(&cookie), Some(body))).await }
    };

    // 學生開始對話（綁定活動與第一個題目 t1）
    let conv = start_from_activity(&app, &a.student, &id).await;

    // 已有對話後：移除已有對話的題目 t1 -> 409
    let res = patch(json!({"topic_ids": [&t2, &t3]})).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(res).await["error"]["code"],
        "activity_topic_locked"
    );

    // 已有對話後：更換已有對話的題目 t1 成 t3 -> 409
    let res = patch(json!({"topic_ids": [&t3, &t2]})).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(res).await["error"]["code"],
        "activity_topic_locked"
    );

    // 已有對話後：重排已有對話的題目（換到後面）-> 409
    let res = patch(json!({"topic_ids": [&t2, &t1]})).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(res).await["error"]["code"],
        "activity_topic_locked"
    );

    // 已有對話後：新增題目成功（在後面追加 t3）-> 200 OK
    let res = patch(json!({"topic_ids": [&t1, &t2, &t3]})).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_body(res).await["topic_ids"], json!([&t1, &t2, &t3]));

    // 再追加 t4 也成功
    let res = patch(json!({"topic_ids": [&t1, &t2, &t3, &t4]})).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        json_body(res).await["topic_ids"],
        json!([&t1, &t2, &t3, &t4])
    );

    // 學生刪除該場對話後，鎖定解除，可以自由重排與更換
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

    let res = patch(json!({"topic_ids": [&t4, &t3]})).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_body(res).await["topic_ids"], json!([&t4, &t3]));
}
