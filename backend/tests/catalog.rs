//! F-02 活動、F-03 題目庫、F-04 學生可選項目：行為與授權測試（合成資料）。

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

fn ids(v: &Value, key: &str) -> Vec<String> {
    v[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["id"].as_str().unwrap().to_string())
        .collect()
}

#[sqlx::test]
async fn only_admin_manages_topics(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let body = json!({"title": "什麼是正義？", "category": "倫理學"});

    let res = send(
        &app,
        json_req("POST", "/api/topics", Some(&a.admin), Some(body.clone())),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let topic = json_body(res).await;
    assert_eq!(topic["is_active"], true);
    let id = topic["id"].as_str().unwrap();

    for (cookie, expected) in [
        (Some(a.teacher.as_str()), StatusCode::FORBIDDEN),
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        (None, StatusCode::UNAUTHORIZED),
    ] {
        let res = send(
            &app,
            json_req("POST", "/api/topics", cookie, Some(body.clone())),
        )
        .await;
        assert_eq!(
            res.status(),
            if cookie == Some(a.teacher.as_str()) {
                StatusCode::CREATED
            } else {
                expected
            }
        );
        let res = send(
            &app,
            json_req(
                "PATCH",
                &format!("/api/topics/{id}"),
                cookie,
                Some(json!({"title": "x"})),
            ),
        )
        .await;
        assert_eq!(
            res.status(),
            if cookie == Some(a.teacher.as_str()) {
                StatusCode::OK
            } else {
                expected
            }
        );
        let res = send(&app, get("/api/topics", cookie)).await;
        assert_eq!(
            res.status(),
            if cookie == Some(a.teacher.as_str()) {
                StatusCode::OK
            } else {
                expected
            }
        );
    }

    let res = send(
        &app,
        json_req(
            "POST",
            "/api/topics",
            Some(&a.admin),
            Some(json!({"title": "  "})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn deactivated_topic_disappears_for_students(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/topics",
            Some(&a.admin),
            Some(json!({"title": "T1"})),
        ),
    )
    .await;
    let id = json_body(res).await["id"].as_str().unwrap().to_string();

    let avail = json_body(send(&app, get("/api/catalog", Some(&a.student))).await).await;
    assert_eq!(ids(&avail, "topics"), vec![id.clone()]);

    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/topics/{id}"),
            Some(&a.admin),
            Some(json!({"is_active": false})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let avail = json_body(send(&app, get("/api/catalog", Some(&a.student))).await).await;
    assert!(ids(&avail, "topics").is_empty());
    // 管理者仍看得到停用的題目
    let all = json_body(send(&app, get("/api/topics", Some(&a.admin))).await).await;
    assert_eq!(all.as_array().unwrap().len(), 1);

    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/topics/{}", uuid::Uuid::new_v4()),
            Some(&a.admin),
            Some(json!({})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn activity_visibility_follows_publish_and_close(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let topic = create_topic(&app, &a, "電車難題").await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(
                json!({"title": "第一週討論", "description": "請討論電車難題", "topic_id": topic, "classroom_id": a.classroom}),
            ),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let act = json_body(res).await;
    assert_eq!(act["status"], "draft");
    let id = act["id"].as_str().unwrap().to_string();

    let visible = |cookie: String| {
        let app = app.clone();
        async move {
            ids(
                &json_body(send(&app, get("/api/catalog", Some(&cookie))).await).await,
                "activities",
            )
        }
    };
    assert!(visible(a.student.clone()).await.is_empty());

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
    assert_eq!(visible(a.student.clone()).await, vec![id.clone()]);

    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/activities/{id}/close"),
            Some(&a.teacher),
            None,
        ),
    )
    .await;
    assert_eq!(json_body(res).await["status"], "closed");
    assert!(visible(a.student.clone()).await.is_empty());

    // 可重新開放
    send(
        &app,
        json_req(
            "POST",
            &format!("/api/activities/{id}/publish"),
            Some(&a.teacher),
            None,
        ),
    )
    .await;
    assert_eq!(visible(a.student.clone()).await, vec![id]);
}

#[sqlx::test]
async fn a_second_teacher_cannot_edit_the_activity_but_an_admin_can(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    // 第二位教師（助教，權限相同）只能管理自己的教室（S-14）：改不了別人教室的活動
    send(
        &app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(&a.admin),
            Some(json!({"email": "ta@example.com"})),
        ),
    )
    .await;
    let ta = login_as(&app, "sub-ta", "ta@example.com").await;
    let topic = create_topic(&app, &a, "T").await;
    let id = create_activity(&app, &a, "A", &topic).await;

    let edit = |cookie: String| {
        let app = app.clone();
        let path = format!("/api/activities/{id}");
        async move {
            send(
                &app,
                json_req(
                    "PATCH",
                    &path,
                    Some(&cookie),
                    Some(json!({"title": "A2", "description": "新說明"})),
                ),
            )
            .await
            .status()
        }
    };
    assert_eq!(edit(ta).await, StatusCode::NOT_FOUND);
    let read = || async {
        json_body(
            send(
                &app,
                get(&format!("/api/activities/{id}"), Some(&a.teacher)),
            )
            .await,
        )
        .await
    };
    assert_eq!(read().await["title"], "A");

    // 管理者可管理所有教室
    assert_eq!(edit(a.admin.clone()).await, StatusCode::OK);
    let got = read().await;
    assert_eq!(got["title"], "A2");
    assert_eq!(got["description"], "新說明");
}

#[sqlx::test]
async fn activity_validation_and_topic_link(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({"title": ""})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(
                json!({"title": "X", "topic_id": uuid::Uuid::new_v4(), "classroom_id": a.classroom}),
            ),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(res).await["error"]["code"], "invalid_topic");
    // 活動必須有題目（S-04.7）：沒帶、帶 null 都不行
    for body in [
        json!({"title": "X"}),
        json!({"title": "X", "topic_id": null}),
    ] {
        let res = send(
            &app,
            json_req("POST", "/api/activities", Some(&a.teacher), Some(body)),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(res).await["error"]["code"], "invalid_topic");
    }

    let topic = json_body(
        send(
            &app,
            json_req(
                "POST",
                "/api/topics",
                Some(&a.admin),
                Some(json!({"title": "T"})),
            ),
        )
        .await,
    )
    .await;
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/activities",
            Some(&a.teacher),
            Some(json!({"title": "X", "topic_id": topic["id"], "classroom_id": a.classroom})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    assert_eq!(json_body(res).await["topic_id"], topic["id"]);

    let missing = uuid::Uuid::new_v4();
    let res = send(
        &app,
        get(&format!("/api/activities/{missing}"), Some(&a.teacher)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/activities/{missing}/publish"),
            Some(&a.teacher),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn activity_management_is_teacher_only(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let topic = create_topic(&app, &a, "T").await;
    let id = create_activity(&app, &a, "A", &topic).await;

    for (cookie, expected) in [
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        (None, StatusCode::UNAUTHORIZED),
    ] {
        for (method, path, body) in [
            ("GET", "/api/activities".to_string(), None),
            (
                "POST",
                "/api/activities".to_string(),
                Some(json!({"title": "Z"})),
            ),
            (
                "PATCH",
                format!("/api/activities/{id}"),
                Some(json!({"title": "Z"})),
            ),
            ("POST", format!("/api/activities/{id}/publish"), None),
            ("POST", format!("/api/activities/{id}/close"), None),
        ] {
            let res = send(&app, json_req(method, &path, cookie, body)).await;
            assert_eq!(res.status(), expected, "{method} {path}");
        }
    }
}

#[sqlx::test]
async fn available_list_requires_enrollment(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let res = send(&app, get("/api/catalog", Some(&a.outsider))).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(res).await["error"]["code"], "not_enrolled");
    assert_eq!(
        send(&app, get("/api/catalog", None)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    // 教師與管理者不參與討論：即使不在名單內也不能用學生功能
    for cookie in [&a.teacher, &a.admin] {
        let res = send(&app, get("/api/catalog", Some(cookie))).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(json_body(res).await["error"]["code"], "forbidden");
    }

    // 被移出名單後立即失去存取
    send(
        &app,
        json_req(
            "DELETE",
            &format!("/api/classrooms/{}/roster/student@example.com", a.classroom),
            Some(&a.admin),
            None,
        ),
    )
    .await;
    let res = send(&app, get("/api/catalog", Some(&a.student))).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn legacy_available_route_is_removed(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    assert_eq!(
        send(&app, get("/api/available", Some(&a.student)))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
async fn activity_topic_is_locked_once_students_have_conversations(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let t1 = create_topic(&app, &a, "電車難題").await;
    let t2 = create_topic(&app, &a, "說謊可以嗎").await;
    let act = publish_activity(&app, &a, "第一週", &t1).await;
    let path = format!("/api/activities/{act}");
    let patch = |body: Value| {
        let app = app.clone();
        let path = path.clone();
        let cookie = a.teacher.clone();
        async move { send(&app, json_req("PATCH", &path, Some(&cookie), Some(body))).await }
    };

    // 還沒有人開始對話：可以換題目
    let res = patch(json!({"topic_id": t2})).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_body(res).await["topic_id"], t2.as_str());
    patch(json!({"topic_id": t1})).await;

    let conv = start_from_activity(&app, &a.student, &act).await;

    // 已有對話：不能換成不同題目，題目不變
    let res = patch(json!({"topic_id": t2})).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(res).await["error"]["code"],
        "activity_topic_locked"
    );
    let got = json_body(send(&app, get(&path, Some(&a.teacher))).await).await;
    assert_eq!(got["topic_id"], t1.as_str());

    // 其他欄位與帶同一個題目仍可修改
    let res = patch(json!({"title": "改名", "topic_id": t1})).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_body(res).await["title"], "改名");

    // 該場對話被刪除後，題目又可以更換
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
    assert_eq!(
        patch(json!({"topic_id": t2})).await.status(),
        StatusCode::OK
    );
}
