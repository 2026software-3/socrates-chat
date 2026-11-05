//! F-30.2 學號可見範圍（S-01.4、S-02.2）：該教室的教師、學生本人與管理者看得到學號（管理者不遮蔽）；
//! 其他教室的教師與其他學生看不到，送往 AI 服務的請求也不含學號。只使用合成資料。
//! 欄位命名依 #446：學號是 `student_number`，學生的使用者 UUID 是 `student_user_id`。

mod common;

use std::time::Duration;

use axum::{Router, http::StatusCode};
use common::Script::*;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// `seed_actors` 匯入的預設學生（`student@example.com`）的合成學號。
const STUDENT_NUMBER: &str = "10000000A";
/// 同一間教室另一位學生的合成學號；匯入時用小寫，確認儲存與顯示都是大寫。
const OTHER_NUMBER: &str = "20000000B";

async fn add_teacher(app: &Router, admin: &str, email: &str) {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(admin),
            Some(json!({"email": email})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
}

/// 第二位學生加入同一間教室並登入（夾具替帳號填上信箱，讓他能用 Google 登入）。
async fn add_other_student(app: &Router, a: &Actors) -> String {
    let line = format!("同學,{},other@example.com", OTHER_NUMBER.to_lowercase());
    let res = import_roster(app, &a.teacher, &a.classroom, &line).await;
    assert_eq!(res.status(), StatusCode::OK);
    login_as(app, "sub-other", "other@example.com").await
}

/// 直接寫入一份已完成的總結（不經過 AI），並讓對話結束。
async fn finish(pool: &PgPool, conversation: &str) {
    let id: Uuid = conversation.parse().unwrap();
    sqlx::query(
        "INSERT INTO summaries (conversation_id, status, stance, reasons, claim)
         VALUES ($1, 'ready', '立場', '理由', '主張')",
    )
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("UPDATE conversations SET status = 'ended', ended_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
}

async fn ok_json(app: &Router, path: &str, cookie: &str) -> Value {
    let res = send(app, get(path, Some(cookie))).await;
    assert_eq!(res.status(), StatusCode::OK, "{path}");
    json_body(res).await
}

fn items(body: &Value) -> &Vec<Value> {
    body["items"].as_array().unwrap()
}

#[sqlx::test]
async fn students_see_only_their_own_student_number(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let other = add_other_student(&app, &a).await;

    assert_eq!(
        ok_json(&app, "/api/me", &a.student).await["student_number"],
        STUDENT_NUMBER
    );
    assert_eq!(
        ok_json(&app, "/api/me", &other).await["student_number"],
        OTHER_NUMBER
    );
    // 教師與管理者的帳號沒有學號
    for cookie in [&a.teacher, &a.admin] {
        assert!(ok_json(&app, "/api/me", cookie).await["student_number"].is_null());
    }

    // 學生能用的端點都不帶出同學的學號
    for path in [
        "/api/me",
        "/api/conversations",
        "/api/dashboard/me",
        "/api/catalog",
        "/api/points",
        "/api/me/usage",
    ] {
        let body = ok_json(&app, path, &a.student).await.to_string();
        assert!(
            !body.contains(OTHER_NUMBER),
            "{path} leaks a classmate's student number"
        );
    }
    // 名單、學生帳號與總結列表都只給教師與管理者
    for path in [
        format!("/api/classrooms/{}/roster", a.classroom),
        format!("/api/classrooms/{}/students", a.classroom),
        "/api/summaries".to_string(),
    ] {
        let res = send(&app, get(&path, Some(&a.student))).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "{path}");
        assert!(!text_body(res).await.contains(OTHER_NUMBER), "{path}");
    }
}

/// 學號屬於帳號、與角色無關：學生的信箱後來被加進教師名單，同時具備教師與學生身分時，
/// `/api/me` 仍回傳自己的學號（只會是自己的，不會外露他人資料）。
#[sqlx::test]
async fn a_student_who_is_also_a_teacher_still_sees_their_own_student_number(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    add_teacher(&app, &a.admin, "student@example.com").await;

    let me = ok_json(&app, "/api/me", &a.student).await;
    assert_eq!(me["roles"]["teacher"], true);
    assert_eq!(me["roles"]["student"], true);
    assert_eq!(me["student_number"], STUDENT_NUMBER);
}

#[sqlx::test]
async fn classroom_teacher_and_admin_see_names_and_upper_case_student_numbers(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    add_other_student(&app, &a).await;

    for cookie in [&a.teacher, &a.admin] {
        let roster = ok_json(
            &app,
            &format!("/api/classrooms/{}/roster", a.classroom),
            cookie,
        )
        .await;
        let row = items(&roster)
            .iter()
            .find(|r| r["student_number"] == OTHER_NUMBER)
            .expect("roster lists the student number in upper case");
        assert_eq!(row["display_name"], "同學");
        assert!(
            items(&roster).iter().any(|r| {
                r["student_number"] == STUDENT_NUMBER && r["display_name"] == "學生"
            })
        );

        let students = ok_json(
            &app,
            &format!("/api/classrooms/{}/students", a.classroom),
            cookie,
        )
        .await;
        let numbers: Vec<&str> = items(&students)
            .iter()
            .map(|r| r["student_number"].as_str().unwrap())
            .collect();
        assert!(
            numbers.contains(&STUDENT_NUMBER) && numbers.contains(&OTHER_NUMBER),
            "{numbers:?}"
        );
    }
}

#[sqlx::test]
async fn summary_list_shows_the_student_number_to_the_classroom_teacher_and_unmasked_to_admins(
    pool: PgPool,
) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    finish(&pool, &conv).await;

    let teacher = ok_json(&app, "/api/summaries", &a.teacher).await;
    let row = &items(&teacher)[0];
    assert_eq!(row["student_number"], STUDENT_NUMBER);
    // 使用者 UUID 是 `student_user_id`，`?student=` 篩選沿用它
    assert!(Uuid::parse_str(row["student_user_id"].as_str().unwrap()).is_ok());

    let admin = ok_json(&app, "/api/summaries", &a.admin).await;
    let row = &items(&admin)[0];
    assert_eq!(
        row["student_number"], STUDENT_NUMBER,
        "admins see it unmasked"
    );
    assert_eq!(
        row["stance"], "message",
        "other sensitive fields stay masked"
    );
}

#[sqlx::test]
async fn other_teachers_outsiders_and_anonymous_users_cannot_see_student_numbers(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    add_other_student(&app, &a).await;
    let conv = start_conversation(&app, &a).await;
    finish(&pool, &conv).await;
    add_teacher(&app, &a.admin, "t2@example.com").await;
    let other_teacher = login_as(&app, "sub-t2", "t2@example.com").await;

    // 其他教師：與不存在的教室相同的 404，不外露教室是否存在
    for path in [
        format!("/api/classrooms/{}/roster", a.classroom),
        format!("/api/classrooms/{}/students", a.classroom),
    ] {
        let res = send(&app, get(&path, Some(&other_teacher))).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{path}");
        let body = text_body(res).await;
        assert!(
            !body.contains(STUDENT_NUMBER) && !body.contains(OTHER_NUMBER),
            "{path}"
        );
    }
    let summaries = ok_json(&app, "/api/summaries", &other_teacher).await;
    assert!(items(&summaries).is_empty());

    for (cookie, status) in [
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        (None, StatusCode::UNAUTHORIZED),
    ] {
        for path in [
            format!("/api/classrooms/{}/roster", a.classroom),
            format!("/api/classrooms/{}/students", a.classroom),
            "/api/summaries".to_string(),
        ] {
            let res = send(&app, get(&path, cookie)).await;
            assert_eq!(res.status(), status, "{path}");
            let body = text_body(res).await;
            assert!(
                !body.contains(STUDENT_NUMBER) && !body.contains(OTHER_NUMBER),
                "{path}"
            );
        }
    }
    // 名單外帳號的 /api/me 也沒有學號
    assert!(ok_json(&app, "/api/me", &a.outsider).await["student_number"].is_null());
}

const REPLY: [&str; 2] = [
    "為什麼你認為應該拉桿？\n<<<META>>>\n",
    "{\"question_type\":\"reason\",\"advance\":false,\"reason\":\"尚未說明理由\"}",
];

const SUMMARY: &str = r#"{"stance": "會拉桿", "reasons": "五個人比一個人多", "claim": "應該拉桿"}"#;

/// 總結與主張分組都在背景任務完成；等分組結果寫進資料庫才算完成。
async fn wait_for_claim_group(pool: &PgPool) {
    for _ in 0..400 {
        let grouped: bool = sqlx::query_scalar(
            "SELECT COALESCE(bool_and(claim_group_id IS NOT NULL), false) FROM summaries",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if grouped {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("claim grouping did not finish");
}

#[sqlx::test]
async fn requests_to_the_ai_service_never_carry_the_student_number(pool: PgPool) {
    let ai = FakeAi::with(vec![
        Reply(REPLY.to_vec()),
        Reply(vec![SUMMARY]),
        Reply(vec!["{\"group\":\"拉桿\"}"]),
    ]);
    let app = test_app_with_ai(pool.clone(), ai.clone());
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;

    let (status, _) = student_says(&app, &a, &conv, "我覺得應該拉桿").await;
    assert_eq!(status, StatusCode::OK);
    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/conversations/{conv}/end"),
            Some(&a.student),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    wait_for_claim_group(&pool).await;

    let reqs = ai.requests.lock().unwrap().clone();
    assert_eq!(reqs.len(), 3, "chat reply, summary and claim grouping");
    for req in &reqs {
        let mut text = req.system.to_uppercase();
        for turn in &req.messages {
            text.push('\n');
            text.push_str(&turn.content.to_uppercase());
        }
        assert!(
            !text.contains(STUDENT_NUMBER),
            "the student number was sent to the AI"
        );
    }
}
