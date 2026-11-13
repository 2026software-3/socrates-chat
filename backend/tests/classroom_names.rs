//! F-31.2 學生端的教室名稱（S-14）：目錄的活動帶教室名稱，我的對話帶來源教室名稱與封存旗標。
//! 學生只看得到自己所在教室的名稱，不含教師、教室說明或其他學生（合成資料）。

mod common;

use axum::{Router, http::StatusCode};
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

/// 教師建立一間有說明的教室，回傳 id。
async fn classroom_with_description(app: &Router, a: &Actors, name: &str, desc: &str) -> String {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/classrooms",
            Some(&a.teacher),
            Some(json!({"name": name, "description": desc})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    json_body(res).await["id"].as_str().unwrap().to_string()
}

/// 在指定教室建立並發布活動，回傳活動 id。
async fn published_in(app: &Router, a: &Actors, classroom: &str, title: &str) -> String {
    let topic = create_topic(app, a, title).await;
    let res = send(
        app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({
                "title": title,
                "description": "你會拉桿嗎？",
                "topic_ids": [topic],
                "classroom_id": classroom,
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let act = json_body(res).await["id"].as_str().unwrap().to_string();
    let res = send(
        app,
        json_req(
            "POST",
            &format!("/api/activities/{act}/publish"),
            Some(&a.teacher),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    act
}

async fn enroll_student(app: &Router, a: &Actors, classroom: &str) {
    let res = import_roster(
        app,
        &a.admin,
        classroom,
        "學生,10000000A,student@example.com",
    )
    .await;
    assert!(res.status().is_success());
}

async fn catalog(app: &Router, cookie: &str) -> Value {
    let res = send(app, get("/api/catalog", Some(cookie))).await;
    assert_eq!(res.status(), StatusCode::OK);
    json_body(res).await
}

async fn my_conversations(app: &Router, cookie: &str, archived: bool) -> Value {
    let res = send(
        app,
        get(
            &format!("/api/conversations?archived={archived}"),
            Some(cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    json_body(res).await["items"].clone()
}

/// 依 id 找出列表中的一筆。
fn by_id<'a>(items: &'a Value, id: &str) -> &'a Value {
    items
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == id)
        .unwrap_or_else(|| panic!("{id} 不在列表中"))
}

async fn personal_practice(app: &Router, a: &Actors) -> String {
    let topic = create_topic(app, a, "說謊可以嗎").await;
    let res = send(
        app,
        json_req(
            "POST",
            "/api/conversations",
            Some(&a.student),
            Some(json!({"topic_id": topic})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    json_body(res).await["id"].as_str().unwrap().to_string()
}

async fn archive_classroom(app: &Router, a: &Actors, classroom: &str) {
    let res = send(
        app,
        json_req(
            "PATCH",
            &format!("/api/classrooms/{classroom}"),
            Some(&a.teacher),
            Some(json!({"is_archived": true})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[sqlx::test]
async fn catalog_activities_carry_the_name_of_each_classroom(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let second = classroom_with_description(&app, &a, "B 班", "教室說明").await;
    enroll_student(&app, &a, &second).await;
    let first_act = published_in(&app, &a, &a.classroom, "電車難題").await;
    let second_act = published_in(&app, &a, &second, "說謊").await;

    let activities = catalog(&app, &a.student).await["activities"].clone();
    assert_eq!(by_id(&activities, &first_act)["classroom_name"], "測試教室");
    assert_eq!(by_id(&activities, &second_act)["classroom_name"], "B 班");
    assert_eq!(by_id(&activities, &second_act)["classroom_id"], second);
}

#[sqlx::test]
async fn students_never_see_names_of_classrooms_they_are_not_in(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let other = classroom_with_description(&app, &a, "別班-秘密名稱", "別班說明").await;
    published_in(&app, &a, &other, "別班活動").await;
    let mine = classroom_with_description(&app, &a, "我的班", "只給教師看的說明").await;
    enroll_student(&app, &a, &mine).await;
    published_in(&app, &a, &mine, "我的活動").await;

    let body = catalog(&app, &a.student).await;
    let raw = body.to_string();
    assert!(!raw.contains("別班-秘密名稱"));
    // 只回名稱：不含教室說明、教師或其他學生資訊（S-14）
    assert!(!raw.contains("只給教師看的說明"));
    assert!(!raw.contains("teacher@example.com"));
    for act in body["activities"].as_array().unwrap() {
        assert!(act.get("classroom_description").is_none());
    }

    // 名單外帳號尚未開通，拿不到目錄
    let res = send(&app, get("/api/catalog", Some(&a.outsider))).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn my_conversations_show_the_source_classroom_or_personal_practice(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let second = classroom_with_description(&app, &a, "B 班", "教室說明").await;
    enroll_student(&app, &a, &second).await;
    let in_first = start_conversation(&app, &a).await;
    let act = published_in(&app, &a, &second, "說謊").await;
    let in_second = start_from_activity(&app, &a.student, &act).await;
    let personal = personal_practice(&app, &a).await;

    let items = my_conversations(&app, &a.student, false).await;
    assert_eq!(by_id(&items, &in_first)["classroom_name"], "測試教室");
    assert_eq!(by_id(&items, &in_first)["classroom_archived"], false);
    assert_eq!(by_id(&items, &in_second)["classroom_name"], "B 班");
    // 個人練習沒有教室：名稱為 null，前端顯示「個人練習」
    assert!(by_id(&items, &personal)["classroom_name"].is_null());
    assert_eq!(by_id(&items, &personal)["classroom_archived"], false);
    assert!(!items.to_string().contains("教室說明"));

    // 開始對話與單場對話的回應同樣帶教室名稱
    let res = send(
        &app,
        get(&format!("/api/conversations/{in_second}"), Some(&a.student)),
    )
    .await;
    assert_eq!(json_body(res).await["classroom_name"], "B 班");
}

#[sqlx::test]
async fn archived_classrooms_keep_their_name_and_are_flagged(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    archive_classroom(&app, &a, &a.classroom).await;

    let items = my_conversations(&app, &a.student, false).await;
    let row = by_id(&items, &conv);
    assert_eq!(row["classroom_name"], "測試教室");
    assert_eq!(row["classroom_archived"], true);
}

#[sqlx::test]
async fn deleting_a_classroom_leaves_no_sourceless_conversations(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    // 學生還在另一間教室，刪除後仍可使用
    let second = classroom_with_description(&app, &a, "B 班", "教室說明").await;
    enroll_student(&app, &a, &second).await;
    start_conversation(&app, &a).await;
    let personal = personal_practice(&app, &a).await;
    let res = send(
        &app,
        json_req(
            "DELETE",
            &format!("/api/classrooms/{}", a.classroom),
            Some(&a.admin),
            Some(json!({"confirm_name": "測試教室"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let items = my_conversations(&app, &a.student, false).await;
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["id"], personal);
    assert!(items[0]["classroom_name"].is_null());
}

#[sqlx::test]
async fn renaming_a_classroom_updates_the_source_of_existing_conversations(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let conv = start_conversation(&app, &a).await;
    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/classrooms/{}", a.classroom),
            Some(&a.teacher),
            Some(json!({"name": "改名後的教室"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    // 名稱是讀取時取自教室，不是建立對話時複製的
    let items = my_conversations(&app, &a.student, false).await;
    assert_eq!(by_id(&items, &conv)["classroom_name"], "改名後的教室");
}

/// 待確認（PR #499）：學生被移出名單後，自己在那間教室的舊對話仍顯示教室名稱。
/// S-14 只規定移出後失去該教室的學生權限、資料保留；這裡固定目前的行為，
/// 之後若決定要隱藏，改這個測試，避免查詢調整時行為無聲改變。
#[sqlx::test]
async fn removed_students_still_see_the_source_of_their_own_conversations(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    // 學生還在另一間教室，移出後仍可使用
    let second = classroom_with_description(&app, &a, "B 班", "教室說明").await;
    enroll_student(&app, &a, &second).await;
    let conv = start_conversation(&app, &a).await;
    let res = send(
        &app,
        json_req(
            "DELETE",
            &format!("/api/classrooms/{}/roster/student@example.com", a.classroom),
            Some(&a.teacher),
            None,
        ),
    )
    .await;
    assert!(res.status().is_success());

    let items = my_conversations(&app, &a.student, false).await;
    assert_eq!(by_id(&items, &conv)["classroom_name"], "測試教室");
    // 但移出的教室不再出現在目錄
    assert!(
        !catalog(&app, &a.student)
            .await
            .to_string()
            .contains("測試教室")
    );
}

#[sqlx::test]
async fn the_create_response_matches_the_conversation_read_back(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let second = classroom_with_description(&app, &a, "B 班", "教室說明").await;
    enroll_student(&app, &a, &second).await;
    let act = published_in(&app, &a, &second, "說謊").await;

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
    let created = json_body(res).await;
    let id = created["id"].as_str().unwrap();
    assert_eq!(created["classroom_name"], "B 班");
    assert_eq!(created["classroom_archived"], false);

    // 建立回應與稍後讀回的對話使用同一份欄位定義，兩者完全一致
    let res = send(
        &app,
        get(&format!("/api/conversations/{id}"), Some(&a.student)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let read = json_body(res).await;
    for field in [
        "classroom_id",
        "classroom_name",
        "classroom_archived",
        "activity_title",
        "updated_at",
        "title",
    ] {
        assert_eq!(created[field], read[field], "{field}");
    }
}
