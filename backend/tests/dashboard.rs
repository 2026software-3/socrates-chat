//! F-15 班上論點分布、F-24 個人儀表板、教師題庫與內建題目：行為與授權（假 AI、合成資料）。

mod common;

use std::time::Duration;

use axum::{Router, http::StatusCode};
use common::Script::*;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

const AGREE: &str = "{\"stance\":\"會拉桿\",\"reasons\":\"救五人\",\"claim\":\"該拉桿\"}";
const DISAGREE: &str = "{\"stance\":\"不拉\",\"reasons\":\"不能殺人\",\"claim\":\"不拉桿\"}";
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

/// 總結完成後，背景還會把主張歸群；等到歸群完成（沒有主張的、個人練習的不用等），讓 AI 腳本的順序固定。
async fn settle_grouping(pool: &PgPool, conv: &str) {
    let id: uuid::Uuid = conv.parse().unwrap();
    for _ in 0..100 {
        let (done,): (bool,) = sqlx::query_as(
            "SELECT s.claim IS NULL OR s.claim_group_id IS NOT NULL OR c.activity_id IS NULL
             FROM summaries s JOIN conversations c ON c.id = s.conversation_id
             WHERE s.conversation_id = $1",
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
async fn finished_conversation(app: &Router, cookie: &str, source: Value, pool: &PgPool) -> String {
    let res = send(
        app,
        json_req("POST", "/api/conversations", Some(cookie), Some(source)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let conv = json_body(res).await["id"].as_str().unwrap().to_string();
    seed_student_message(pool, &conv, "我的想法").await;
    send(
        app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/end"),
            Some(cookie),
            None,
        ),
    )
    .await;
    done_summary(app, cookie, &conv).await;
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

struct Seeded {
    app: Router,
    a: Actors,
    ai: std::sync::Arc<FakeAi>,
    act1: String,
    act2: String,
}

/// 兩個活動：第一個活動兩位學生各完成一場（立場相反），第二個活動一位學生完成一場（沒有主張）；
/// 另外還有一場從題目庫開始的個人練習（不屬於任何活動）。
async fn seeded(pool: PgPool) -> Seeded {
    // 每場對話：先是總結，有主張時接著一次歸群（個人練習不歸群）
    let ai = FakeAi::with(vec![
        Reply(vec![AGREE]),
        Reply(vec![GROUP_PULL]),
        Reply(vec![DISAGREE]),
        Reply(vec![GROUP_NOT_PULL]),
        Reply(vec![NO_CLASS]),
        Reply(vec![AGREE]),
    ]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let a = seed_actors(&app).await;
    let student2 = add_student(&app, &a, "sub-student2", "student2@example.com").await;
    let act1 = published_activity_with_topic(&app, &a, "電車難題").await;
    let act2 = published_activity_with_topic(&app, &a, "說謊可以嗎").await;
    finished_conversation(&app, &a.student, json!({"activity_id": act1}), &pool).await;
    finished_conversation(&app, &student2, json!({"activity_id": act1}), &pool).await;
    finished_conversation(&app, &a.student, json!({"activity_id": act2}), &pool).await;
    let practice = create_topic(&app, &a.teacher, "自由意志").await;
    finished_conversation(&app, &a.student, json!({"topic_id": practice}), &pool).await;
    Seeded {
        app,
        a,
        ai,
        act1,
        act2,
    }
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
    let avail = json_body(send(&app, get("/api/catalog", Some(&a.student))).await).await;
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

    let avail = json_body(send(&app, get("/api/catalog", Some(&a.student))).await).await;
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
async fn summary_has_a_claim_but_no_school_classification(pool: PgPool) {
    let Seeded { app, a, .. } = seeded(pool).await;
    let list = json_body(send(&app, get("/api/teacher/summaries", Some(&a.teacher))).await).await;
    let claims: Vec<Value> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["claim"].clone())
        .collect();
    assert!(claims.contains(&json!("不拉桿")) && claims.contains(&json!("該拉桿")));
    assert!(claims.contains(&Value::Null), "沒有主張的總結照常列出");
    for r in list.as_array().unwrap() {
        assert!(r.get("framework").is_none() && r.get("framework_scores").is_none());
    }
    // 學生本人的總結也沒有學派分類與 6 維分數（S-04.6）
    let conv = json_body(send(&app, get("/api/conversations", Some(&a.student))).await).await;
    let id = conv["items"][0]["id"].as_str().unwrap();
    let s = json_body(
        send(
            &app,
            get(
                &format!("/api/conversations/{id}/summary"),
                Some(&a.student),
            ),
        )
        .await,
    )
    .await;
    assert!(s.get("framework").is_none() && s.get("framework_scores").is_none());
}

#[sqlx::test]
async fn class_distribution_is_per_activity_and_ignores_practice(pool: PgPool) {
    let Seeded {
        app,
        a,
        ai,
        act1,
        act2,
    } = seeded(pool).await;
    let d = json_body(send(&app, get("/api/dashboard/class", Some(&a.teacher))).await).await;
    assert_eq!(d["masked"], false);
    assert_eq!(d["students_total"], 2);
    assert_eq!(d["students_participating"], 2);
    assert_eq!(d["completed"], 3, "個人練習不列入教師的彙整");
    // 學派人數與雷達圖已取消（S-04.6）
    assert!(d.get("frameworks").is_none() && d.get("radar").is_none());

    let activities = d["activities"].as_array().unwrap();
    assert_eq!(activities.len(), 2);
    assert_eq!(activities[0]["activity_id"], act1.as_str());
    assert_eq!(activities[0]["title"], "電車難題", "完成場數多的在前");
    assert_eq!(activities[0]["completed"], 2);
    assert!(activities[0].get("radar").is_none());
    // 主張分布：兩個不同主張各一群；沒有主張的對話不列入
    let claims = activities[0]["claims"].as_array().unwrap();
    let names: Vec<&str> = claims.iter().map(|g| g["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["不拉桿", "拉桿"]);
    assert!(claims[0].get("radar").is_none());
    assert_eq!(activities[1]["activity_id"], act2.as_str());
    assert_eq!(activities[1]["completed"], 1);
    assert!(activities[1]["claims"].as_array().unwrap().is_empty());
    assert_eq!(activities[1]["ungrouped"], 0);
    // 3 場活動對話各有一次總結，其中兩場有主張而歸群；個人練習只有總結、不歸群
    assert_eq!(ai.request_count(), 6);
}

#[sqlx::test]
async fn class_distribution_is_teacher_only_and_hidden_from_admins(pool: PgPool) {
    let Seeded { app, a, .. } = seeded(pool).await;
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
    assert!(d["activities"].as_array().unwrap().is_empty());
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
        for path in ["/api/catalog", "/api/conversations", "/api/dashboard/me"] {
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
    let Seeded { app, a, .. } = seeded(pool).await;
    let d = json_body(send(&app, get("/api/dashboard/me", Some(&a.student))).await).await;
    // 學生本人 3 場（含個人練習），另一位學生的不算
    assert_eq!(d["total_conversations"], 3);
    assert_eq!(d["total_turns"], 3);
    assert_eq!(d["completed"], 3);
    // 個人雷達圖與學派人數已取消（S-04.6）
    assert!(d.get("radar").is_none() && d.get("frameworks").is_none());
    let recent = d["recent"].as_array().unwrap();
    assert_eq!(recent.len(), 3);
    assert!(recent[0]["title"].is_string());
    assert!(recent[0]["stance"].is_string());
    for r in recent {
        assert!(r.get("framework").is_none() && r.get("framework_scores").is_none());
    }

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
