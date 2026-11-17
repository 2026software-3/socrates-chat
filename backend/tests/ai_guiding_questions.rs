//! F-32.5（S-04.8 修訂）：對話提示、總結、論證樹、雷達評分改按順序帶入題目的引導問題，
//! 不再讀 `description`。假 AI 記錄實際送出的請求，合成資料。

mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::{Router, http::StatusCode};
use common::Script::*;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const OLD_DESCRIPTION: &str = "舊說明DESC不該再出現";
const GOOD_SUMMARY: &str = "{\"stance\":\"會拉桿\",\"reasons\":\"救五人優先\"}";
const SCORES: &str = r#"{"d1":5,"d2":0,"d3":3,"d4":1,"d5":0,"d6":2}"#;
const TREE: &str = r#"{"topic_id":"TREE","title":"電車難題","nodes":[{"id":"TREE-C0","kind":"claim","label":"會拉桿","source_round":1,"evidence":"我會拉桿。","parent":null,"relation":null,"origin":"user"}]}"#;

struct Fixture {
    app: Router,
    ai: Arc<FakeAi>,
    actors: Actors,
    topic: String,
    conversation: String,
}

/// 題目帶兩個引導問題與一段舊說明；學生從活動開始一場對話。
async fn fixture(pool: &PgPool, questions: &[&str], scripts: Vec<Script>) -> Fixture {
    let ai = FakeAi::with(scripts);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let actors = seed_actors(&app).await;
    let topic: Uuid = sqlx::query_scalar(
        "INSERT INTO topics (title, description, guiding_questions) VALUES ('電車難題', $1, $2) RETURNING id",
    )
    .bind(OLD_DESCRIPTION)
    .bind(questions.iter().map(|q| q.to_string()).collect::<Vec<_>>())
    .fetch_one(pool)
    .await
    .unwrap();
    let topic = topic.to_string();
    let activity = publish_activity(&app, &actors, "電車難題", &topic).await;
    // 明確帶 topic_id：對話的說明才是從題目複製的（只帶活動時取的是活動說明）
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/conversations",
            Some(&actors.student),
            Some(json!({"activity_id": activity, "topic_id": topic})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let conversation = json_body(res).await["id"].as_str().unwrap().to_string();
    Fixture {
        app,
        ai,
        actors,
        topic,
        conversation,
    }
}

fn assert_lists_in_order(text: &str, questions: &[&str]) {
    let mut last = 0;
    for q in questions {
        let at = text
            .find(q)
            .unwrap_or_else(|| panic!("提示裡找不到引導問題「{q}」"));
        assert!(at >= last, "引導問題「{q}」的順序不對");
        last = at;
    }
}

async fn end_and_settle(f: &Fixture, pool: &PgPool) {
    seed_student_message(pool, &f.conversation, "我會拉桿。").await;
    let res = send(
        &f.app,
        json_req(
            "POST",
            &format!("/api/conversations/{}/end", f.conversation),
            Some(&f.actors.student),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    settled_summary(&f.app, &f.actors.student, &f.conversation).await;
}

#[sqlx::test]
async fn chat_prompt_lists_the_guiding_questions_in_order_and_not_the_description(pool: PgPool) {
    let f = fixture(
        &pool,
        &["問題甲：前提是什麼？", "問題乙：有反例嗎？"],
        vec![Reply(vec!["好"])],
    )
    .await;
    student_says(&f.app, &f.actors, &f.conversation, "我會拉桿").await;

    let requests = f.ai.requests.lock().unwrap();
    let system = &requests[0].system;
    assert_lists_in_order(system, &["問題甲：前提是什麼？", "問題乙：有反例嗎？"]);
    assert!(
        !system.contains(OLD_DESCRIPTION),
        "對話提示不該再帶 description"
    );
}

#[sqlx::test]
async fn chat_prompt_for_a_topic_without_guiding_questions_has_only_title_and_category(
    pool: PgPool,
) {
    let f = fixture(&pool, &[], vec![Reply(vec!["好"])]).await;
    student_says(&f.app, &f.actors, &f.conversation, "我會拉桿").await;

    let requests = f.ai.requests.lock().unwrap();
    let system = &requests[0].system;
    assert!(system.contains("電車難題"));
    assert!(!system.contains(OLD_DESCRIPTION));
}

#[sqlx::test]
async fn chat_prompt_follows_the_current_guiding_questions_after_the_teacher_edits_them(
    pool: PgPool,
) {
    let f = fixture(&pool, &["舊問題"], vec![Reply(vec!["好"])]).await;
    let res = send(
        &f.app,
        json_req(
            "PATCH",
            &format!("/api/topics/{}", f.topic),
            Some(&f.actors.teacher),
            Some(json!({"guiding_questions": ["新問題"]})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    student_says(&f.app, &f.actors, &f.conversation, "我會拉桿").await;

    let requests = f.ai.requests.lock().unwrap();
    assert!(requests[0].system.contains("新問題"));
    assert!(!requests[0].system.contains("舊問題"));
}

#[sqlx::test]
async fn summary_prompt_lists_the_guiding_questions_and_not_the_description(pool: PgPool) {
    let f = fixture(
        &pool,
        &["問題甲", "問題乙"],
        vec![Reply(vec![GOOD_SUMMARY])],
    )
    .await;
    end_and_settle(&f, &pool).await;

    let requests = f.ai.requests.lock().unwrap();
    let system = &requests[0].system;
    assert_lists_in_order(system, &["問題甲", "問題乙"]);
    assert!(!system.contains(OLD_DESCRIPTION));
}

#[sqlx::test]
async fn argument_tree_input_carries_guiding_questions_and_not_the_description(pool: PgPool) {
    let f = fixture(
        &pool,
        &["問題甲", "問題乙"],
        vec![Reply(vec![GOOD_SUMMARY]), Reply(vec![TREE])],
    )
    .await;
    end_and_settle(&f, &pool).await;
    let path = format!("/api/conversations/{}/argument-tree", f.conversation);
    let res = send(
        &f.app,
        json_req("POST", &path, Some(&f.actors.student), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    for _ in 0..100 {
        if f.ai.request_count() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let requests = f.ai.requests.lock().unwrap();
    let content = &requests[1].messages[0].content;
    let payload: Value = serde_json::from_str(content).unwrap();
    assert_eq!(payload["guiding_questions"], json!(["問題甲", "問題乙"]));
    assert!(!content.contains(OLD_DESCRIPTION));
}

#[sqlx::test]
async fn radar_scoring_input_carries_guiding_questions_and_not_the_description(pool: PgPool) {
    let f = fixture(
        &pool,
        &["問題甲", "問題乙"],
        vec![Reply(vec![GOOD_SUMMARY]), Reply(vec![SCORES])],
    )
    .await;
    let dimensions: Vec<Value> = (1..=6)
        .map(|n| {
            json!({
                "id": format!("d{n}"),
                "name": {"zh_tw": format!("傾向{n}"), "en": format!("T{n}"), "es": format!("T{n}")},
                "description": {"zh_tw": format!("說明{n}"), "en": format!("A{n}"), "es": format!("S{n}")},
                "rubric": (1..=5).map(|l| format!("第{l}級")).collect::<Vec<_>>(),
            })
        })
        .collect();
    sqlx::query(
        "UPDATE topics SET dimensions = $2, dimensions_prompt_version = 'topic-dimensions-v1',
                dimensions_language = 'zh-TW' WHERE id = $1",
    )
    .bind(f.topic.parse::<Uuid>().unwrap())
    .bind(sqlx::types::Json(dimensions))
    .execute(&pool)
    .await
    .unwrap();
    end_and_settle(&f, &pool).await;
    for _ in 0..200 {
        if f.ai.request_count() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let requests = f.ai.requests.lock().unwrap();
    let content = &requests[1].messages[0].content;
    let payload: Value = serde_json::from_str(content).unwrap();
    assert_eq!(
        payload["topic"]["guiding_questions"],
        json!(["問題甲", "問題乙"])
    );
    assert!(!content.contains(OLD_DESCRIPTION));
}
