//! F-32.1 題目的引導問題：資料模型與題目 API（S-04.8）
//! 題目可附 0 到 5 個引導問題，每個最多 200 字；已有對話後仍可修改。

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::json;
use socrates_chat_backend::catalog::{TROLLEY_TOPIC_ID, seed_builtin_topics};
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
                "title": "笛卡兒的懷疑論"
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

#[sqlx::test]
async fn padded_guiding_question_is_length_checked_and_stored_trimmed(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;

    // 去掉前後空白後恰為 200 字，應通過；儲存的也必須是去空白後的內容
    let question = "字".repeat(200);
    let padded = format!("{}{question}{}", " ".repeat(50), " ".repeat(50));
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/topics",
            Some(&a.teacher),
            Some(json!({"title": "空白題目", "guiding_questions": [padded]})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let created = json_body(res).await;
    assert_eq!(created["guiding_questions"], json!([question]));

    // 去空白後仍超過 200 字，依舊回 400（空白不能讓上限失效，也不能讓合法內容被誤拒）
    let too_long = format!("{}{}", " ".repeat(50), "字".repeat(201));
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/topics",
            Some(&a.teacher),
            Some(json!({"title": "超長題目", "guiding_questions": [too_long]})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn builtin_trolley_topic_is_seeded_with_guiding_question_and_no_description(pool: PgPool) {
    // 全新資料庫的內建題目直接寫入引導問題（S-04.8 修訂：description 不再寫入）
    seed_builtin_topics(&pool).await.unwrap();
    let (description, questions): (String, Vec<String>) =
        sqlx::query_as("SELECT description, guiding_questions FROM topics WHERE id = $1")
            .bind(TROLLEY_TOPIC_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(description, "");
    assert_eq!(questions.len(), 1);
    assert!(questions[0].contains("電車"));
}

#[sqlx::test]
async fn topic_translation_uses_display_language_and_preserves_manual_edits(pool: PgPool) {
    let ai = FakeAi::with(vec![]);
    ai.topic_translation_scripts
        .lock()
        .unwrap()
        .push_back(Script::Reply(vec![r#""A choice""#]));
    let app = test_app_with_ai(pool, ai.clone());
    let actors = seed_actors(&app).await;
    let created = send(
        &app,
        json_req(
            "POST",
            "/api/topics",
            Some(&actors.teacher),
            Some(json!({
                "title": "選擇", "guiding_questions": ["為什麼？"],
                "translations": {"en": {"q:為什麼？": "Why?"}}
            })),
        ),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let topic = json_body(created).await;
    let id = topic["id"].as_str().unwrap();
    assert_eq!(topic["translations"]["en"]["title"], "A choice");
    for (lang, title, pending) in [("en", "A choice", false), ("es", "選擇", true)] {
        let catalog = json_body(
            send(
                &app,
                get(
                    &format!("/api/catalog?language={lang}"),
                    Some(&actors.student),
                ),
            )
            .await,
        )
        .await;
        let localized = catalog["topics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == id)
            .unwrap();
        assert_eq!(localized["title"], title);
        assert_eq!(localized["title_pending"], pending);
        assert_eq!(
            localized["pending_questions"],
            if pending { json!([0]) } else { json!([]) }
        );
    }
    let created = json_body(
        send(
            &app,
            json_req(
                "POST",
                "/api/conversations",
                Some(&actors.student),
                Some(json!({"topic_id":id})),
            ),
        )
        .await,
    )
    .await;
    let list = json_body(
        send(
            &app,
            get("/api/conversations?language=en", Some(&actors.student)),
        )
        .await,
    )
    .await;
    assert_eq!(list["items"][0]["title"], "A choice");
    assert_eq!(list["items"][0]["language"], created["language"]);
    let switch = send(
        &app,
        json_req(
            "PATCH",
            "/api/me",
            Some(&actors.student),
            Some(json!({"language":"en"})),
        ),
    )
    .await;
    assert!(switch.status().is_success());
    let detail_path = format!("/api/conversations/{}", created["id"].as_str().unwrap());
    let detail = json_body(send(&app, get(&detail_path, Some(&actors.student))).await).await;
    assert_eq!(detail["language"], "zh-TW");
    assert_eq!(detail["guiding_questions"], json!(["為什麼？"]));

    let path = format!("/api/topics/{id}");
    let correction = json!({"translations":{"en":{"title":"Choosing"}}});
    for cookie in [Some(&actors.student), Some(&actors.outsider)] {
        let denied = send(
            &app,
            json_req(
                "PATCH",
                &path,
                cookie.map(String::as_str),
                Some(correction.clone()),
            ),
        )
        .await;
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    }
    assert_eq!(
        send(&app, get("/api/catalog", None)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let en_requests = || {
        ai.topic_translation_requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.system.contains("into en."))
            .count()
    };
    let before = en_requests();
    for patch in [correction, json!({"title":"選擇","is_active":true})] {
        let result = json_body(
            send(
                &app,
                json_req("PATCH", &path, Some(&actors.teacher), Some(patch)),
            )
            .await,
        )
        .await;
        assert_eq!(result["translations"]["en"]["title"], "Choosing");
    }
    assert_eq!(en_requests(), before);
}

#[sqlx::test(migrations = false)]
async fn existing_topic_and_conversation_survive_translation_migration(pool: PgPool) {
    let all = sqlx::migrate!();
    let before = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all.iter()
                .filter(|m| m.version < 20261121000100)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    before.run(&pool).await.unwrap();
    let id: uuid::Uuid = sqlx::query_scalar("INSERT INTO topics(title, guiding_questions) VALUES ('選擇', ARRAY['為什麼？']) RETURNING id").fetch_one(&pool).await.unwrap();
    let app = test_app(pool.clone());
    let actors = seed_actors(&app).await;
    // Create pre-migration data with the pre-migration schema. The current API requires
    // current columns; calling it here would not represent an existing conversation.
    let conversation: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO conversations(user_id, topic_id, title)
         SELECT id, $1, '選擇' FROM users WHERE email='student@example.com' RETURNING id",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    all.run(&pool).await.unwrap();
    let list = json_body(
        send(
            &app,
            get("/api/conversations?language=es", Some(&actors.student)),
        )
        .await,
    )
    .await;
    assert_eq!(list["items"][0]["id"], conversation.to_string());
    assert_eq!(list["items"][0]["title"], "選擇");
    assert_eq!(list["items"][0]["title_pending"], true);
    let catalog =
        json_body(send(&app, get("/api/catalog?language=es", Some(&actors.student))).await).await;
    let topic = catalog["topics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == id.to_string())
        .unwrap();
    assert_eq!(topic["guiding_questions"], json!(["為什麼？"]));
    assert_eq!(topic["pending_questions"], json!([0]));
}

#[sqlx::test]
async fn conversation_list_keeps_saved_title_after_topic_rename(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let id = create_topic(&app, &a, "舊題名").await;
    let conversation = json_body(
        send(
            &app,
            json_req(
                "POST",
                "/api/conversations",
                Some(&a.student),
                Some(json!({"topic_id": id})),
            ),
        )
        .await,
    )
    .await;
    let renamed = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/topics/{id}"),
            Some(&a.teacher),
            Some(json!({"title": "新題名"})),
        ),
    )
    .await;
    assert_eq!(renamed.status(), StatusCode::OK);
    let list = json_body(send(&app, get("/api/conversations", Some(&a.student))).await).await;
    let detail_path = format!(
        "/api/conversations/{}",
        conversation["id"].as_str().unwrap()
    );
    let detail = json_body(send(&app, get(&detail_path, Some(&a.student))).await).await;
    assert_eq!(list["items"][0]["title"], "舊題名");
    assert_eq!(list["items"][0]["title"], detail["title"]);
}

#[sqlx::test]
async fn missing_translation_is_retried_on_later_patch(pool: PgPool) {
    let ai = FakeAi::with(vec![]);
    let app = test_app_with_ai(pool, ai.clone());
    let a = seed_actors(&app).await;
    let topic = json_body(
        send(
            &app,
            json_req(
                "POST",
                "/api/topics",
                Some(&a.teacher),
                Some(json!({"title": "選擇"})),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(topic["translations"]["en"], json!({}));
    let id = topic["id"].as_str().unwrap();
    for _ in 0..2 {
        ai.topic_translation_scripts
            .lock()
            .unwrap()
            .push_back(Script::Reply(vec![r#""Choice""#]));
    }
    let patched = json_body(
        send(
            &app,
            json_req(
                "PATCH",
                &format!("/api/topics/{id}"),
                Some(&a.teacher),
                Some(json!({"description": "x"})),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(patched["translations"]["en"]["title"], "Choice");
    assert_eq!(patched["translations"]["es"]["title"], "Choice");
}

#[sqlx::test]
async fn concurrent_patch_during_translation_is_not_rejected(pool: PgPool) {
    // 慢的那次 PATCH 的翻譯被閘門擋住，直到快的那次完全寫入才放行：先後順序由事件決定，不靠時間。
    // AI 總逾時拉長，閘門等多久都不會被逾時截斷。
    let ai = FakeAi::with(vec![]);
    let mut config = test_config();
    config.ai_total_timeout = std::time::Duration::from_secs(60);
    let app = test_app_with_config(pool, ai.clone(), config);
    let a = seed_actors(&app).await;
    let id = create_topic(&app, &a, "選擇").await;
    let (release, gate) = tokio::sync::watch::channel(false);
    // 標題只有一欄，英文與西班牙文各一次翻譯請求，兩個腳本剛好都給慢的那次
    for _ in 0..2 {
        ai.topic_translation_scripts
            .lock()
            .unwrap()
            .push_back(Script::Gated(vec![r#""New""#], gate.clone()));
    }
    let already_requested = ai.topic_translation_requests.lock().unwrap().len();
    let path = format!("/api/topics/{id}");
    let slow = send(
        &app,
        json_req(
            "PATCH",
            &path,
            Some(&a.teacher),
            Some(json!({"title": "新題名"})),
        ),
    );
    let fast = async {
        // 慢的那次已讀完快照並送出兩個翻譯請求（都卡在閘門），才送快的那次
        wait_until_topic_translations_requested(&ai, already_requested + 2).await;
        let response = send(
            &app,
            json_req(
                "PATCH",
                &path,
                Some(&a.teacher),
                Some(json!({"guiding_questions": ["為什麼？"]})),
            ),
        )
        .await;
        // 快的那次已經寫入，慢的那次才繼續
        release.send(true).unwrap();
        response
    };
    let (slow, fast) = tokio::join!(slow, fast);
    assert_eq!(fast.status(), StatusCode::OK);
    assert_eq!(slow.status(), StatusCode::OK);
    let topic = json_body(slow).await;
    assert_eq!(topic["title"], "新題名");
    // 慢的那次寫入前發現題目已被快的那次改過，要保留快的那次的問題
    assert_eq!(topic["guiding_questions"], json!(["為什麼？"]));
}

#[sqlx::test]
async fn conversation_detail_localizes_title_without_switching_conversation_language(pool: PgPool) {
    let app = test_app(pool.clone());
    let actors = seed_actors(&app).await;
    let conversation = start_conversation(&app, &actors).await;
    sqlx::query(
        "UPDATE topics SET translations = '{\"en\":{\"title\":\"A translated topic\"}}'
                 WHERE id = (SELECT topic_id FROM conversations WHERE id = $1::uuid)",
    )
    .bind(&conversation)
    .execute(&pool)
    .await
    .unwrap();
    let result = json_body(
        send(
            &app,
            get(
                &format!("/api/conversations/{conversation}?language=en"),
                Some(&actors.student),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(result["title"], "A translated topic");
    assert_eq!(result["language"], "zh-TW");
}

#[sqlx::test]
async fn backfills_existing_learning_content_and_preserves_manual_translation(pool: PgPool) {
    use socrates_chat_backend::{AppState, app, topic_translations::fill_missing};
    use std::sync::Arc;

    let ai = FakeAi::with(
        (0..4)
            .map(|_| Script::Reply(vec![r#""Translated activity text""#]))
            .collect(),
    );
    ai.topic_translation_scripts
        .lock()
        .unwrap()
        .extend((0..3).map(|_| Script::Reply(vec![r#""Translated topic text""#])));
    register_fixture_pool(&pool);
    let state = AppState::new(
        pool.clone(),
        test_config(),
        Arc::new(FakeIdentity),
        ai.clone(),
    );
    let app = app(state.clone());
    let actors = seed_actors(&app).await;
    let topic: uuid::Uuid = sqlx::query_scalar(
        r#"INSERT INTO topics (title, guiding_questions, translations)
           VALUES ('舊題名', ARRAY['舊問題？'], '{"en":{"title":"Teacher title"}}') RETURNING id"#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let activity = publish_activity(&app, &actors, "舊活動", &topic.to_string()).await;
    sqlx::query("UPDATE activities SET description = '舊活動說明' WHERE id = $1::uuid")
        .bind(&activity)
        .execute(&pool)
        .await
        .unwrap();

    fill_missing(&state).await.unwrap();
    let staff_list =
        json_body(send(&app, get("/api/activities", Some(&actors.teacher))).await).await;
    assert_eq!(staff_list["items"][0]["title"], "舊活動");
    assert_eq!(
        staff_list["items"][0]["translations"]["en"]["title"],
        "Translated activity text"
    );
    assert_eq!(
        staff_list["items"][0]["translations"]["en"]["description"],
        "Translated activity text"
    );
    let catalog =
        json_body(send(&app, get("/api/catalog?language=en", Some(&actors.student))).await).await;
    assert_eq!(catalog["topics"][0]["title"], "Teacher title");
    assert_eq!(
        catalog["topics"][0]["guiding_questions"],
        json!(["Translated topic text"])
    );
    assert_eq!(
        catalog["activities"][0]["title"],
        "Translated activity text"
    );
    assert_eq!(
        catalog["activities"][0]["description"],
        "Translated activity text"
    );
    assert_eq!(catalog["activities"][0]["description_pending"], false);

    let count = ai.request_count();
    let topic_count = ai.topic_translation_requests.lock().unwrap().len();
    fill_missing(&state).await.unwrap();
    assert_eq!(ai.request_count(), count);
    assert_eq!(
        ai.topic_translation_requests.lock().unwrap().len(),
        topic_count
    );

    let response = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/activities/{activity}"),
            Some(&actors.teacher),
            Some(json!({ "description": "更新說明" })),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let catalog =
        json_body(send(&app, get("/api/catalog?language=es", Some(&actors.student))).await).await;
    assert_eq!(
        catalog["activities"][0]["title"],
        "Translated activity text"
    );
    assert_eq!(catalog["activities"][0]["description"], "更新說明");
    assert_eq!(catalog["activities"][0]["description_pending"], true);
}

fn failing_translation_state(
    pool: &PgPool,
) -> (socrates_chat_backend::AppState, std::sync::Arc<FakeAi>) {
    use socrates_chat_backend::AppState;
    use std::sync::Arc;

    let ai = FakeAi::with(Vec::new());
    ai.topic_translation_scripts
        .lock()
        .unwrap()
        .extend((0..200).map(|_| Script::Fail));
    register_fixture_pool(pool);
    let state = AppState::new(
        pool.clone(),
        test_config(),
        Arc::new(FakeIdentity),
        ai.clone(),
    );
    (state, ai)
}

#[sqlx::test]
async fn rows_whose_translation_keeps_failing_are_not_retried_on_the_next_pass(pool: PgPool) {
    use socrates_chat_backend::topic_translations::{TranslationRetries, fill_missing_with};

    let (state, ai) = failing_translation_state(&pool);
    sqlx::query(
        "INSERT INTO topics (title, guiding_questions) VALUES ('舊題名', ARRAY['舊問題？'])",
    )
    .execute(&pool)
    .await
    .unwrap();

    let mut retries = TranslationRetries::default();
    fill_missing_with(&state, &mut retries).await.unwrap();
    let attempted = ai.topic_translation_requests.lock().unwrap().len();
    assert!(
        attempted > 0,
        "the first pass tries the missing translations"
    );

    fill_missing_with(&state, &mut retries).await.unwrap();
    assert_eq!(
        ai.topic_translation_requests.lock().unwrap().len(),
        attempted,
        "a row that just failed waits out its backoff"
    );
}

#[sqlx::test]
async fn failed_language_does_not_rewrite_the_row_with_an_empty_entry(pool: PgPool) {
    use socrates_chat_backend::topic_translations::fill_missing;

    let (state, _ai) = failing_translation_state(&pool);
    let stored = r#"{"en":{"title":"T","q:舊問題？":"Q"}}"#;
    let topic: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO topics (title, guiding_questions, translations)
         VALUES ('舊題名', ARRAY['舊問題？'], $1::jsonb) RETURNING id",
    )
    .bind(stored)
    .fetch_one(&pool)
    .await
    .unwrap();

    fill_missing(&state).await.unwrap();
    let after: serde_json::Value =
        sqlx::query_scalar("SELECT translations FROM topics WHERE id = $1")
            .bind(topic)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        after,
        serde_json::from_str::<serde_json::Value>(stored).unwrap()
    );
}
