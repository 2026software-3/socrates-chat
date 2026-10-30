//! F-28.3 活動與對話歸屬教室（S-14、S-02.1）。只使用合成資料。

mod common;

use axum::{Router, http::StatusCode};
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const NOPE: &str = "00000000-0000-4000-8000-000000000000";

async fn add_teacher(app: &Router, admin: &str, sub: &str, email: &str) -> String {
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
    login_as(app, sub, email).await
}

async fn new_activity(
    app: &Router,
    cookie: &str,
    classroom: &str,
    topic: &str,
    title: &str,
) -> (StatusCode, Value) {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/activities",
            Some(cookie),
            Some(json!({"title": title, "topic_id": topic, "classroom_id": classroom})),
        ),
    )
    .await;
    (res.status(), json_body(res).await)
}

async fn publish(app: &Router, cookie: &str, activity: &str) -> StatusCode {
    send(
        app,
        json_req(
            "POST",
            &format!("/api/activities/{activity}/publish"),
            Some(cookie),
            None,
        ),
    )
    .await
    .status()
}

async fn start(app: &Router, cookie: &str, body: Value) -> (StatusCode, Value) {
    let res = send(
        app,
        json_req("POST", "/api/conversations", Some(cookie), Some(body)),
    )
    .await;
    (res.status(), json_body(res).await)
}

/// 直接寫入一份已完成的總結（不經過 AI），並讓對話結束。
async fn finish(pool: &PgPool, conversation: &str, claim: &str) {
    let id: Uuid = conversation.parse().unwrap();
    sqlx::query(
        "INSERT INTO summaries (conversation_id, status, stance, reasons, claim, completed_at)
         VALUES ($1, 'ready', '立場', '理由', $2, now())",
    )
    .bind(id)
    .bind(claim)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("UPDATE conversations SET status = 'ended', ended_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
}

async fn list(app: &Router, path: &str, cookie: &str) -> Value {
    let res = send(app, get(path, Some(cookie))).await;
    assert_eq!(res.status(), StatusCode::OK, "{path}");
    // 分頁列表的項目在 items；其餘端點直接回傳
    let body = json_body(res).await;
    body.get("items").cloned().unwrap_or(body)
}

fn titles(rows: &Value) -> Vec<String> {
    let mut out: Vec<String> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["title"].as_str().unwrap().to_string())
        .collect();
    out.sort();
    out
}

// ---- 活動屬於教室 ----

#[sqlx::test]
async fn an_activity_belongs_to_one_classroom_and_other_teachers_cannot_reach_it(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let t2 = add_teacher(&app, &a.admin, "sub-t2", "teacher2@example.com").await;
    let topic = create_topic(&app, &a, "電車難題").await;

    let (status, activity) = new_activity(&app, &a.teacher, &a.classroom, &topic, "活動甲").await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(activity["classroom_id"], a.classroom);
    let id = activity["id"].as_str().unwrap();

    // 其他教師看不到、也改不了；與不存在的活動回應相同
    let mine = list(&app, "/api/activities", &a.teacher).await;
    assert_eq!(titles(&mine), ["活動甲"]);
    assert!(
        list(&app, "/api/activities", &t2)
            .await
            .as_array()
            .unwrap()
            .is_empty()
    );
    for path in [id, NOPE] {
        let res = send(&app, get(&format!("/api/activities/{path}"), Some(&t2))).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "get {path}");
        for (method, suffix, body) in [
            ("PATCH", "", Some(json!({"title": "偷改"}))),
            ("POST", "/publish", None),
            ("POST", "/close", None),
        ] {
            let res = send(
                &app,
                json_req(
                    method,
                    &format!("/api/activities/{path}{suffix}"),
                    Some(&t2),
                    body,
                ),
            )
            .await;
            assert_eq!(
                res.status(),
                StatusCode::NOT_FOUND,
                "{method} {path}{suffix}"
            );
        }
    }
    let after = json_body(
        send(
            &app,
            get(&format!("/api/activities/{id}"), Some(&a.teacher)),
        )
        .await,
    )
    .await;
    assert_eq!(after["title"], "活動甲");
    assert_eq!(after["status"], "draft");

    // 管理者可管理所有教室的活動
    assert_eq!(publish(&app, &a.admin, id).await, StatusCode::OK);
}

#[sqlx::test]
async fn creating_an_activity_requires_a_classroom_the_teacher_manages(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let t2 = add_teacher(&app, &a.admin, "sub-t2", "teacher2@example.com").await;
    let topic = create_topic(&app, &a, "電車難題").await;

    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({"title": "沒教室", "topic_id": topic})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(res).await["error"]["code"], "invalid_classroom");

    // 他人的教室與不存在的教室回應相同
    let (s1, b1) = new_activity(&app, &t2, &a.classroom, &topic, "闖入").await;
    let (s2, b2) = new_activity(&app, &t2, NOPE, &topic, "闖入").await;
    assert_eq!((s1, s2), (StatusCode::NOT_FOUND, StatusCode::NOT_FOUND));
    assert_eq!(b1["error"]["code"], b2["error"]["code"]);

    // 學生、名單外與未登入不行
    for (cookie, expected) in [
        (&a.student, StatusCode::FORBIDDEN),
        (&a.outsider, StatusCode::FORBIDDEN),
    ] {
        let (s, _) = new_activity(&app, cookie, &a.classroom, &topic, "x").await;
        assert_eq!(s, expected);
    }
    assert!(
        list(&app, "/api/activities", &a.teacher)
            .await
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[sqlx::test]
async fn two_classrooms_share_one_topic_but_keep_separate_activities(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let room_b = create_classroom(&app, &a.teacher, "B 班").await;
    let topic = create_topic(&app, &a, "電車難題").await;

    let (_, in_a) = new_activity(&app, &a.teacher, &a.classroom, &topic, "A 的活動").await;
    let (_, in_b) = new_activity(&app, &a.teacher, &room_b, &topic, "B 的活動").await;
    let (id_a, id_b) = (in_a["id"].as_str().unwrap(), in_b["id"].as_str().unwrap());
    assert_eq!(in_a["topic_id"], in_b["topic_id"]);
    let topics: i64 = sqlx::query_scalar("SELECT count(*) FROM topics WHERE title = '電車難題'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(topics, 1, "題目只有一份");

    // 各教室的活動設定互不影響
    assert_eq!(publish(&app, &a.teacher, id_a).await, StatusCode::OK);
    let b = json_body(
        send(
            &app,
            get(&format!("/api/activities/{id_b}"), Some(&a.teacher)),
        )
        .await,
    )
    .await;
    assert_eq!(b["status"], "draft");
    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/activities/{id_a}"),
            Some(&a.teacher),
            Some(json!({"title": "改過"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let b = json_body(
        send(
            &app,
            get(&format!("/api/activities/{id_b}"), Some(&a.teacher)),
        )
        .await,
    )
    .await;
    assert_eq!(b["title"], "B 的活動");

    // 可依教室篩選；篩選到他人的教室與不存在的教室回應相同
    let only_b = list(
        &app,
        &format!("/api/activities?classroom_id={room_b}"),
        &a.teacher,
    )
    .await;
    assert_eq!(titles(&only_b), ["B 的活動"]);
    let t2 = add_teacher(&app, &a.admin, "sub-t2", "teacher2@example.com").await;
    for room in [a.classroom.as_str(), NOPE] {
        let res = send(
            &app,
            get(&format!("/api/activities?classroom_id={room}"), Some(&t2)),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
}

// ---- 對話的教室歸屬 ----

#[sqlx::test]
async fn an_activity_conversation_keeps_the_activitys_classroom_until_the_activity_is_deleted(
    pool: PgPool,
) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let topic = create_topic(&app, &a, "電車難題").await;
    let (_, activity) = new_activity(&app, &a.teacher, &a.classroom, &topic, "活動甲").await;
    let act = activity["id"].as_str().unwrap();
    publish(&app, &a.teacher, act).await;

    let (status, conv) = start(&app, &a.student, json!({"activity_id": act})).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(conv["classroom_id"], a.classroom);
    let conv_id = conv["id"].as_str().unwrap();

    // 停用活動後歸屬不變；刪除活動則連同其對話一併刪除（S-14）
    send(
        &app,
        json_req(
            "POST",
            &format!("/api/activities/{act}/close"),
            Some(&a.teacher),
            None,
        ),
    )
    .await;
    let classroom_id: Option<Uuid> =
        sqlx::query_scalar("SELECT classroom_id FROM conversations WHERE id = $1")
            .bind(conv_id.parse::<Uuid>().unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(classroom_id, Some(a.classroom.parse().unwrap()));
    sqlx::query("DELETE FROM activities WHERE id = $1")
        .bind(act.parse::<Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM conversations WHERE id = $1")
        .bind(conv_id.parse::<Uuid>().unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(remaining, 0);
}

#[sqlx::test]
async fn personal_practice_has_no_classroom(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let topic = create_topic(&app, &a, "電車難題").await;

    let (status, conv) = start(&app, &a.student, json!({"topic_id": topic})).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(conv["classroom_id"], Value::Null);
    assert_eq!(conv["activity_id"], Value::Null);
}

#[sqlx::test]
async fn a_student_outside_the_activitys_classroom_is_rejected_like_a_missing_activity(
    pool: PgPool,
) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let t2 = add_teacher(&app, &a.admin, "sub-t2", "teacher2@example.com").await;
    let room_b = create_classroom(&app, &t2, "別人的教室").await;
    let topic = create_topic(&app, &a, "電車難題").await;
    let (_, activity) = new_activity(&app, &t2, &room_b, &topic, "別班活動").await;
    let act = activity["id"].as_str().unwrap();
    publish(&app, &t2, act).await;

    // 學生只在 A 班：看不到、也不能從別班的活動開始對話，且回應和不存在的活動相同
    let catalog = list(&app, "/api/catalog", &a.student).await;
    assert!(catalog["activities"].as_array().unwrap().is_empty());
    let (s1, b1) = start(&app, &a.student, json!({"activity_id": act})).await;
    let (s2, b2) = start(&app, &a.student, json!({"activity_id": NOPE})).await;
    assert_eq!((s1, s2), (StatusCode::BAD_REQUEST, StatusCode::BAD_REQUEST));
    assert_eq!(b1["error"]["code"], "invalid_conversation_source");
    assert_eq!(b1["error"]["code"], b2["error"]["code"]);

    // 加進 B 班名單後就可以；A 班仍在，所以兩邊的活動都看得到
    import_roster(&app, &t2, &room_b, "student@example.com").await;
    let (_, mine) = new_activity(&app, &a.teacher, &a.classroom, &topic, "本班活動").await;
    publish(&app, &a.teacher, mine["id"].as_str().unwrap()).await;
    let catalog = list(&app, "/api/catalog", &a.student).await;
    assert_eq!(titles(&catalog["activities"]), ["別班活動", "本班活動"]);
    let (status, conv) = start(&app, &a.student, json!({"activity_id": act})).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(conv["classroom_id"], room_b);

    // 移出 B 班名單後，不能再從 B 班活動開始新的對話
    send(
        &app,
        json_req(
            "DELETE",
            &format!("/api/classrooms/{room_b}/roster/student@example.com"),
            Some(&t2),
            None,
        ),
    )
    .await;
    let (_, mine2) = new_activity(&app, &t2, &room_b, &topic, "B 班第二個").await;
    publish(&app, &t2, mine2["id"].as_str().unwrap()).await;
    let (status, _) = start(&app, &a.student, json!({"activity_id": mine2["id"]})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn students_see_their_activity_and_practice_conversations(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let topic = create_topic(&app, &a, "電車難題").await;
    let act = publish_activity(&app, &a, "活動甲", &topic).await;
    let (_, with_activity) = start(&app, &a.student, json!({"activity_id": act})).await;
    let (_, practice) = start(&app, &a.student, json!({"topic_id": topic})).await;

    let page = list(&app, "/api/conversations", &a.student).await;
    let mut ids: Vec<&str> = page
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    ids.sort();
    let mut expected = vec![
        with_activity["id"].as_str().unwrap(),
        practice["id"].as_str().unwrap(),
    ];
    expected.sort();
    assert_eq!(ids, expected);
}

// ---- 教師端查詢經過教室 ----

struct Scene {
    a: Actors,
    other_teacher: String,
    room_b: String,
    room_other: String,
}

/// A 班（教師 1）：學生做了活動對話與個人練習；B 班（教師 1）有另一位學生的活動對話；
/// 教師 2 的教室有第三位學生的活動對話。每場對話都已有完成的總結。
async fn scene(app: &Router, pool: &PgPool) -> Scene {
    let a = seed_actors(app).await;
    let other_teacher = add_teacher(app, &a.admin, "sub-t2", "teacher2@example.com").await;
    let topic = create_topic(app, &a, "電車難題").await;
    let room_b = create_classroom(app, &a.teacher, "B 班").await;
    let room_other = create_classroom(app, &other_teacher, "別人的教室").await;

    let b_student = {
        import_roster(app, &a.teacher, &room_b, "bee@example.com").await;
        login_as(app, "sub-bee", "bee@example.com").await
    };
    let o_student = {
        import_roster(app, &other_teacher, &room_other, "oh@example.com").await;
        login_as(app, "sub-oh", "oh@example.com").await
    };

    for (cookie, teacher, room, title, claim) in [
        (
            &a.student,
            &a.teacher,
            a.classroom.as_str(),
            "A 班活動",
            "A 主張",
        ),
        (
            &b_student,
            &a.teacher,
            room_b.as_str(),
            "B 班活動",
            "B 主張",
        ),
        (
            &o_student,
            &other_teacher,
            room_other.as_str(),
            "他班活動",
            "他班主張",
        ),
    ] {
        let (_, activity) = new_activity(app, teacher, room, &topic, title).await;
        let act = activity["id"].as_str().unwrap();
        publish(app, teacher, act).await;
        let (_, conv) = start(app, cookie, json!({"activity_id": act})).await;
        finish(pool, conv["id"].as_str().unwrap(), claim).await;
    }
    let (_, practice) = start(app, &a.student, json!({"topic_id": topic})).await;
    finish(pool, practice["id"].as_str().unwrap(), "私下練習主張").await;
    Scene {
        a,
        other_teacher,
        room_b,
        room_other,
    }
}

fn claims(rows: &Value) -> Vec<String> {
    let mut out: Vec<String> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["claim"].as_str().unwrap().to_string())
        .collect();
    out.sort();
    out
}

#[sqlx::test]
async fn teacher_summaries_only_cover_own_classrooms_and_never_personal_practice(pool: PgPool) {
    let app = test_app(pool.clone());
    let s = scene(&app, &pool).await;

    let rows = list(&app, "/api/summaries", &s.a.teacher).await;
    assert_eq!(claims(&rows), ["A 主張", "B 主張"]);
    assert!(!rows.to_string().contains("私下練習"));
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .all(|r| r["classroom_id"].is_string())
    );

    let rows = list(&app, "/api/summaries", &s.other_teacher).await;
    assert_eq!(claims(&rows), ["他班主張"]);

    // 依教室篩選；他人的教室與不存在的教室回應相同
    let rows = list(
        &app,
        &format!("/api/summaries?classroom_id={}", s.room_b),
        &s.a.teacher,
    )
    .await;
    assert_eq!(claims(&rows), ["B 主張"]);
    for room in [s.room_other.as_str(), NOPE] {
        let res = send(
            &app,
            get(
                &format!("/api/summaries?classroom_id={room}"),
                Some(&s.a.teacher),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    // 管理者看得到所有教室的活動對話（內容遮蔽），同樣看不到個人練習
    let rows = list(&app, "/api/summaries", &s.a.admin).await;
    assert_eq!(rows.as_array().unwrap().len(), 3);
    assert!(!rows.to_string().contains("主張"));
}

#[sqlx::test]
async fn class_dashboard_is_scoped_to_own_classrooms_and_excludes_personal_practice(pool: PgPool) {
    let app = test_app(pool.clone());
    let s = scene(&app, &pool).await;

    let all = list(&app, "/api/dashboard/class", &s.a.teacher).await;
    assert_eq!(all["completed"], 2, "不計入個人練習與他人教室");
    assert_eq!(all["students_participating"], 2);
    assert_eq!(all["students_total"], 2);
    let names: Vec<_> = all["activities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["title"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"A 班活動") && names.contains(&"B 班活動") && names.len() == 2);

    let only_a = list(
        &app,
        &format!("/api/dashboard/class?classroom_id={}", s.a.classroom),
        &s.a.teacher,
    )
    .await;
    assert_eq!(only_a["completed"], 1);
    assert_eq!(only_a["students_total"], 1);
    assert_eq!(only_a["activities"].as_array().unwrap().len(), 1);

    let theirs = list(&app, "/api/dashboard/class", &s.other_teacher).await;
    assert_eq!(theirs["completed"], 1);

    for room in [s.room_other.as_str(), NOPE] {
        let res = send(
            &app,
            get(
                &format!("/api/dashboard/class?classroom_id={room}"),
                Some(&s.a.teacher),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    // 管理者只有人數，沒有分析結果
    let admin = list(
        &app,
        &format!("/api/dashboard/class?classroom_id={}", s.room_b),
        &s.a.admin,
    )
    .await;
    assert_eq!(admin["masked"], true);
    assert_eq!(admin["students_total"], 1);
    assert_eq!(admin["completed"], 0);
}

#[sqlx::test]
async fn teachers_cannot_regroup_another_teachers_activity(pool: PgPool) {
    let app = test_app(pool.clone());
    let s = scene(&app, &pool).await;
    let foreign: Uuid = sqlx::query_scalar("SELECT id FROM activities WHERE title = '他班活動'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/dashboard/class/regroup",
            Some(&s.a.teacher),
            Some(json!({"activity_id": foreign})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/dashboard/class/regroup",
            Some(&s.a.teacher),
            Some(json!({"activity_id": NOPE})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn student_list_counts_only_that_classrooms_completed_conversations(pool: PgPool) {
    let app = test_app(pool.clone());
    let s = scene(&app, &pool).await;
    // 同一位學生也在 B 班
    import_roster(&app, &s.a.teacher, &s.room_b, "student@example.com").await;

    let rows = list(
        &app,
        &format!("/api/classrooms/{}/students", s.a.classroom),
        &s.a.teacher,
    )
    .await;
    let me = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["email"] == "student@example.com")
        .unwrap();
    assert_eq!(me["completed_conversations"], 1, "個人練習不計入教室");

    let rows = list(
        &app,
        &format!("/api/classrooms/{}/students", s.room_b),
        &s.a.teacher,
    )
    .await;
    let me = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["email"] == "student@example.com")
        .unwrap();
    assert_eq!(me["completed_conversations"], 0);
}
