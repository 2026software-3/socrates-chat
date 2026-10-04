//! F-15 班上論點分布、F-24 個人儀表板、教師題庫與內建題目：行為與授權（假 AI、合成資料）。

mod common;

use std::time::Duration;

use axum::{Router, http::StatusCode};
use common::Script::*;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

const AGREE_UTIL: &str = "{\"stance\":\"會拉桿\",\"reasons\":\"救五人\",\"claim\":\"該拉桿\",\"framework\":\"utilitarianism\",\"scores\":{\"utilitarianism\":5,\"deontology\":1,\"virtue\":0,\"contractarianism\":0,\"care\":0,\"existentialism\":0}}";
const DISAGREE_DEONT: &str = "{\"stance\":\"不拉\",\"reasons\":\"不能殺人\",\"claim\":\"不拉桿\",\"framework\":\"deontology\",\"scores\":{\"utilitarianism\":0,\"deontology\":5,\"virtue\":2,\"contractarianism\":0,\"care\":0,\"existentialism\":0}}";
const GROUP_PULL: &str = "{\"group\":\"拉桿\"}";
const GROUP_NOT_PULL: &str = "{\"group\":\"不拉桿\"}";
const NO_CLASS: &str = "{\"stance\":\"還在想\",\"reasons\":\"不確定\",\"turning_points\":\"無\"}";

async fn done_summary(app: &Router, cookie: &str, conv: &str) {
    for _ in 0..100 {
        let res = send(
            app,
            get(&format!("/api/conversations/{conv}/summary"), Some(cookie)),
        )
        .await;
        if json_body(res).await["status"] != "pending" {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("summary stayed pending");
}

/// 總結完成後，背景還會把主張歸群；等到歸群完成（沒有主張的不用等），讓 AI 腳本的順序固定。
async fn settle_grouping(pool: &PgPool, conv: &str) {
    let id: uuid::Uuid = conv.parse().unwrap();
    for _ in 0..100 {
        let (done,): (bool,) = sqlx::query_as(
            "SELECT claim IS NULL OR claim_group_id IS NOT NULL FROM summaries WHERE conversation_id = $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
        if done {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("claim grouping did not finish");
}

/// 開始一場對話、講一句話、結束並等總結與歸群完成。
async fn finished_conversation(app: &Router, a: &Actors, source: Value, pool: &PgPool) -> String {
    let res = send(
        app,
        json_req("POST", "/api/conversations", Some(&a.student), Some(source)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let conv = json_body(res).await["id"].as_str().unwrap().to_string();
    student_says(app, a, &conv, "我的想法").await;
    send(
        app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/end"),
            Some(&a.student),
            None,
        ),
    )
    .await;
    done_summary(app, &a.student, &conv).await;
    settle_grouping(pool, &conv).await;
    conv
}

async fn create_topic(app: &Router, cookie: &str, title: &str) -> String {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/topics",
            Some(cookie),
            Some(json!({"title": title})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    json_body(res).await["id"].as_str().unwrap().to_string()
}

/// 教師建立兩個題目，學生完成三場對話（含一場沒有分類）。
async fn seeded(pool: PgPool) -> (Router, Actors) {
    // 每場對話：先是總結，有主張時接著一次歸群
    let ai = FakeAi::with(vec![
        Reply(vec![AGREE_UTIL]),
        Reply(vec![GROUP_PULL]),
        Reply(vec![DISAGREE_DEONT]),
        Reply(vec![GROUP_NOT_PULL]),
        Reply(vec![NO_CLASS]),
    ]);
    let app = test_app_with_ai(pool.clone(), ai);
    let a = seed_actors(&app).await;
    let t1 = create_topic(&app, &a.teacher, "電車難題").await;
    let t2 = create_topic(&app, &a.teacher, "說謊可以嗎").await;
    finished_conversation(&app, &a, json!({"topic_id": t1}), &pool).await;
    finished_conversation(&app, &a, json!({"topic_id": t1}), &pool).await;
    finished_conversation(&app, &a, json!({"topic_id": t2}), &pool).await;
    (app, a)
}

#[sqlx::test]
async fn teachers_manage_the_topic_bank(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let id = create_topic(&app, &a.teacher, "  電車難題  ").await;

    let list = json_body(send(&app, get("/api/topics", Some(&a.teacher))).await).await;
    assert_eq!(list[0]["title"], "電車難題");
    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/topics/{id}"),
            Some(&a.teacher),
            Some(json!({"is_active": false})),
        ),
    )
    .await;
    assert_eq!(json_body(res).await["is_active"], false);
    let avail = json_body(send(&app, get("/api/available", Some(&a.student))).await).await;
    assert!(avail["topics"].as_array().unwrap().is_empty());

    let blank = send(
        &app,
        json_req(
            "POST",
            "/api/topics",
            Some(&a.teacher),
            Some(json!({"title": " "})),
        ),
    )
    .await;
    assert_eq!(blank.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn topic_bank_is_closed_to_students_and_outsiders(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let id = create_topic(&app, &a.teacher, "T").await;
    for (cookie, status) in [
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        (None, StatusCode::UNAUTHORIZED),
    ] {
        assert_eq!(
            send(&app, get("/api/topics", cookie)).await.status(),
            status
        );
        for (m, p, b) in [
            ("POST", "/api/topics".to_string(), json!({"title": "x"})),
            ("PATCH", format!("/api/topics/{id}"), json!({"title": "x"})),
        ] {
            let res = send(&app, json_req(m, &p, cookie, Some(b))).await;
            assert_eq!(res.status(), status, "{m} {p}");
        }
    }
}

#[sqlx::test]
async fn builtin_trolley_problem_is_seeded_once_and_respects_edits(pool: PgPool) {
    use socrates_chat_backend::catalog::seed_builtin_topics;
    seed_builtin_topics(&pool).await.unwrap();
    seed_builtin_topics(&pool).await.unwrap();
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;

    let avail = json_body(send(&app, get("/api/available", Some(&a.student))).await).await;
    let topics = avail["topics"].as_array().unwrap();
    assert_eq!(topics.len(), 1, "重複啟動不會重複建立");
    assert_eq!(topics[0]["title"], "電車難題");
    assert!(topics[0]["description"].as_str().unwrap().contains("拉桿"));

    // 教師停用並改名後，再次啟動不會被覆寫或復活
    let id = topics[0]["id"].as_str().unwrap();
    send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/topics/{id}"),
            Some(&a.teacher),
            Some(json!({"title": "改過的題目", "is_active": false})),
        ),
    )
    .await;
    seed_builtin_topics(&pool).await.unwrap();
    let all = json_body(send(&app, get("/api/topics", Some(&a.teacher))).await).await;
    assert_eq!(all.as_array().unwrap().len(), 1);
    assert_eq!(all[0]["title"], "改過的題目");
    assert_eq!(all[0]["is_active"], false);
}

#[sqlx::test]
async fn summary_carries_ai_classification_and_tolerates_missing_one(pool: PgPool) {
    let (app, a) = seeded(pool).await;
    let list = json_body(send(&app, get("/api/teacher/summaries", Some(&a.teacher))).await).await;
    let mut pairs: Vec<(Value, Value)> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["claim"].clone(), r["framework"].clone()))
        .collect();
    pairs.sort_by_key(|p| p.0.to_string());
    assert_eq!(
        pairs,
        vec![
            (json!("不拉桿"), json!("deontology")),
            (json!("該拉桿"), json!("utilitarianism")),
            (Value::Null, Value::Null),
        ]
    );
}

#[sqlx::test]
async fn class_distribution_counts_frameworks_and_groups_claims_per_topic(pool: PgPool) {
    let (app, a) = seeded(pool).await;
    let d = json_body(send(&app, get("/api/dashboard/class", Some(&a.teacher))).await).await;
    assert_eq!(d["masked"], false);
    assert_eq!(d["students_total"], 1);
    assert_eq!(d["students_participating"], 1);
    assert_eq!(d["completed"], 3);
    assert_eq!(d["frameworks"]["utilitarianism"], 1);
    assert_eq!(d["frameworks"]["deontology"], 1);
    assert_eq!(d["frameworks"]["care"], 0, "沒人選的分類也列出");
    assert_eq!(d["frameworks"]["unclassified"], 1);

    // 6 維雷達圖：只平均有分數的總結（3 場完成、2 場有分數）
    assert_eq!(d["radar"]["scored"], 2);
    assert_eq!(d["radar"]["max"], 5);
    assert_eq!(d["radar"]["average"]["utilitarianism"], 2.5);
    assert_eq!(d["radar"]["average"]["deontology"], 3.0);
    assert_eq!(d["radar"]["average"]["virtue"], 1.0);
    assert_eq!(d["radar"]["average"]["care"], 0.0);
    let topics = d["topics"].as_array().unwrap();
    assert_eq!(topics.len(), 2);
    assert_eq!(topics[1]["radar"]["scored"], 0, "沒有分數的題目各維度為 0");
    assert_eq!(topics[0]["title"], "電車難題", "完成場數多的在前");
    assert_eq!(topics[0]["completed"], 2);
    // 主張分布：兩個不同主張各一群；沒有主張的對話不列入
    let names: Vec<&str> = topics[0]["claims"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["不拉桿", "拉桿"]);
    assert_eq!(topics[1]["title"], "說謊可以嗎");
    assert!(topics[1]["claims"].as_array().unwrap().is_empty());
    assert_eq!(topics[1]["ungrouped"], 0);
}

#[sqlx::test]
async fn class_distribution_is_teacher_only_and_hidden_from_admins(pool: PgPool) {
    let (app, a) = seeded(pool).await;
    for (cookie, status) in [
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        (None, StatusCode::UNAUTHORIZED),
    ] {
        assert_eq!(
            send(&app, get("/api/dashboard/class", cookie))
                .await
                .status(),
            status
        );
    }
    // 管理者看不到分析結果（S-02.2）
    let d = json_body(send(&app, get("/api/dashboard/class", Some(&a.admin))).await).await;
    assert_eq!(d["masked"], true);
    assert_eq!(d["completed"], 0);
    assert!(d["topics"].as_array().unwrap().is_empty());
    assert_eq!(d["radar"]["scored"], 0);
    assert!(d["radar"]["average"].as_object().unwrap().is_empty());
}

#[sqlx::test]
async fn teachers_and_admins_cannot_take_part_in_discussions(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let t = create_topic(&app, &a.teacher, "T").await;
    for cookie in [&a.teacher, &a.admin] {
        let res = send(
            &app,
            json_req(
                "POST",
                "/api/conversations",
                Some(cookie),
                Some(json!({"topic_id": t})),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        for path in ["/api/available", "/api/conversations", "/api/dashboard/me"] {
            let res = send(&app, get(path, Some(cookie))).await;
            assert_eq!(res.status(), StatusCode::FORBIDDEN, "{path}");
            assert_eq!(json_body(res).await["error"]["code"], "forbidden");
        }
    }
    // 班上分布不受影響：教師照常可看
    let d = json_body(send(&app, get("/api/dashboard/class", Some(&a.teacher))).await).await;
    assert_eq!(d["completed"], 0);
}

#[sqlx::test]
async fn personal_dashboard_counts_only_my_own_conversations(pool: PgPool) {
    let (app, a) = seeded(pool).await;
    let d = json_body(send(&app, get("/api/dashboard/me", Some(&a.student))).await).await;
    assert_eq!(d["total_conversations"], 3);
    assert_eq!(d["total_turns"], 3);
    assert_eq!(d["completed"], 3);
    assert_eq!(d["frameworks"]["deontology"], 1);
    assert_eq!(d["radar"]["scored"], 2);
    assert_eq!(d["radar"]["average"]["deontology"], 3.0);
    let recent = d["recent"].as_array().unwrap();
    assert_eq!(recent.len(), 3);
    assert!(recent[0]["title"].is_string());
    assert!(recent[0]["stance"].is_string());
    assert!(
        recent
            .iter()
            .any(|r| r["framework_scores"]["deontology"] == 5),
        "單場分數也回給學生本人"
    );

    for (cookie, status) in [
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        (None, StatusCode::UNAUTHORIZED),
    ] {
        assert_eq!(
            send(&app, get("/api/dashboard/me", cookie)).await.status(),
            status
        );
    }
}
