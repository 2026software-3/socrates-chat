//! F-32.1 題目的引導問題：資料模型與題目 API（S-04.8）
//! 題目可附 0 到 5 個引導問題，每個最多 200 字；已有對話後仍可修改。

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test]
async fn create_topic_with_guiding_questions_and_default(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;

    // 1. 帶引導問題建立題目
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/topics",
            Some(&a.teacher),
            Some(json!({
                "title": "人工智慧與道德主體",
                "category": "科技倫理",
                "guiding_questions": [
                    "AI 是否具備感受痛苦的能力？",
                    "如果 AI 做出道德抉擇，責任該歸屬於誰？"
                ]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let topic1 = json_body(res).await;
    assert_eq!(
        topic1["guiding_questions"],
        json!([
            "AI 是否具備感受痛苦的能力？",
            "如果 AI 做出道德抉擇，責任該歸屬於誰？"
        ])
    );

    // 2. 未帶 guiding_questions 建立題目，預設為空陣列
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/topics",
            Some(&a.teacher),
            Some(json!({
                "title": "笛卡兒的懷疑論",
                "category": "知識論"
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let topic2 = json_body(res).await;
    assert_eq!(topic2["guiding_questions"], json!([]));

    // 3. GET /api/topics 查詢列表均帶有 guiding_questions
    let res = send(&app, get("/api/topics", Some(&a.teacher))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let topics = json_body(res).await;
    let t1 = topics
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == topic1["id"])
        .unwrap();
    assert_eq!(
        t1["guiding_questions"],
        json!([
            "AI 是否具備感受痛苦的能力？",
            "如果 AI 做出道德抉擇，責任該歸屬於誰？"
        ])
    );
    let t2 = topics
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == topic2["id"])
        .unwrap();
    assert_eq!(t2["guiding_questions"], json!([]));
}

#[sqlx::test]
async fn patch_topic_guiding_questions_semantics(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;

    let res = send(
        &app,
        json_req(
            "POST",
            "/api/topics",
            Some(&a.teacher),
            Some(json!({
                "title": "虛無主義",
                "guiding_questions": ["如果一切沒有意義，為何要活著？"]
            })),
        ),
    )
    .await;
    let id = json_body(res).await["id"].as_str().unwrap().to_string();

    // 1. PATCH 更新引導問題清單
    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/topics/{id}"),
            Some(&a.teacher),
            Some(json!({
                "guiding_questions": [
                    "意義是由客觀賦予還是主觀創造？",
                    "尼采的主動虛無主義是什麼？"
                ]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let patched = json_body(res).await;
    assert_eq!(
        patched["guiding_questions"],
        json!([
            "意義是由客觀賦予還是主觀創造？",
            "尼采的主動虛無主義是什麼？"
        ])
    );

    // 2. PATCH 未帶 guiding_questions 欄位（例如只改標題）應保留原有引導問題
    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/topics/{id}"),
            Some(&a.teacher),
            Some(json!({"title": "存在主義與虛無"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let patched = json_body(res).await;
    assert_eq!(patched["title"], "存在主義與虛無");
    assert_eq!(
        patched["guiding_questions"],
        json!([
            "意義是由客觀賦予還是主觀創造？",
            "尼采的主動虛無主義是什麼？"
        ])
    );

    // 3. PATCH 傳空陣列 [] 應清空引導問題
    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/topics/{id}"),
            Some(&a.teacher),
            Some(json!({"guiding_questions": []})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let patched = json_body(res).await;
    assert_eq!(patched["guiding_questions"], json!([]));
}

#[sqlx::test]
async fn guiding_questions_validation_limits(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;

    // 1. 超過 5 個引導問題回 400
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/topics",
            Some(&a.teacher),
            Some(json!({
                "title": "問題過多題目",
                "guiding_questions": ["Q1", "Q2", "Q3", "Q4", "Q5", "Q6"]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let err = json_body(res).await;
    assert_eq!(err["error"]["code"], "invalid_guiding_questions");

    // 2. 單一問題超過 200 字回 400
    let long_q = "字".repeat(201);
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/topics",
            Some(&a.teacher),
            Some(json!({
                "title": "字數過長題目",
                "guiding_questions": [long_q]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let err = json_body(res).await;
    assert_eq!(err["error"]["code"], "invalid_guiding_questions");

    // 3. 單一問題為空字串或純空白回 400
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/topics",
            Some(&a.teacher),
            Some(json!({
                "title": "空白問題題目",
                "guiding_questions": ["   "]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let err = json_body(res).await;
    assert_eq!(err["error"]["code"], "invalid_guiding_questions");

    // 4. 邊界成功情境：剛好 5 個問題，每個剛好 200 字
    let boundary_q = "正".repeat(200);
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/topics",
            Some(&a.teacher),
            Some(json!({
                "title": "上限剛好合格",
                "guiding_questions": [
                    &boundary_q, &boundary_q, &boundary_q, &boundary_q, &boundary_q
                ]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let valid_topic = json_body(res).await;
    let id = valid_topic["id"].as_str().unwrap();

    // 5. PATCH 同樣受驗證限制（超過 5 個、超過 200 字）
    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/topics/{id}"),
            Some(&a.teacher),
            Some(json!({
                "guiding_questions": ["1", "2", "3", "4", "5", "6"]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(res).await["error"]["code"],
        "invalid_guiding_questions"
    );

    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/topics/{id}"),
            Some(&a.teacher),
            Some(json!({
                "guiding_questions": ["A".repeat(201)]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(res).await["error"]["code"],
        "invalid_guiding_questions"
    );
}

#[sqlx::test]
async fn modify_guiding_questions_with_active_dialogues(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;

    // 建立含引導問題的題目
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/topics",
            Some(&a.teacher),
            Some(json!({
                "title": "已開對話的題目",
                "guiding_questions": ["初始引導問題一"]
            })),
        ),
    )
    .await;
    let topic_id = json_body(res).await["id"].as_str().unwrap().to_string();

    // 建立活動並發布
    let act_id = create_activity(&app, &a, "活動A", &topic_id).await;
    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/activities/{act_id}/publish"),
            Some(&a.teacher),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    // 學生開始對話
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/conversations",
            Some(&a.student),
            Some(json!({"activity_id": act_id, "topic_id": topic_id})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);

    // S-04.8：題目已有對話後仍可修改引導問題（不鎖定）
    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/topics/{topic_id}"),
            Some(&a.teacher),
            Some(json!({
                "guiding_questions": [
                    "修改後的引導問題一",
                    "新增的引導問題二"
                ]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let patched = json_body(res).await;
    assert_eq!(
        patched["guiding_questions"],
        json!(["修改後的引導問題一", "新增的引導問題二"])
    );
}

#[sqlx::test]
async fn student_cannot_modify_guiding_questions_but_can_view_in_catalog(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;

    // 教師建立題目
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/topics",
            Some(&a.teacher),
            Some(json!({
                "title": "學生可讀的題目",
                "guiding_questions": ["問題一", "問題二"]
            })),
        ),
    )
    .await;
    let topic_id = json_body(res).await["id"].as_str().unwrap().to_string();

    // 學生嘗試建立或修改題目回 403
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/topics",
            Some(&a.student),
            Some(json!({
                "title": "學生嘗試建立",
                "guiding_questions": ["Q"]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/topics/{topic_id}"),
            Some(&a.student),
            Some(json!({
                "guiding_questions": ["學生竄改"]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 學生呼叫 GET /api/catalog 可以看到引導問題
    let res = send(&app, get("/api/catalog", Some(&a.student))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let catalog = json_body(res).await;
    let t = catalog["topics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == topic_id)
        .expect("topic found in catalog");
    assert_eq!(t["guiding_questions"], json!(["問題一", "問題二"]));
}
