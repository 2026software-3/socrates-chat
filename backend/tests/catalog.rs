//! F-03 題目、F-04 學生可選題目：行為與授權測試（合成資料）。

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

fn ids(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x["id"].as_str().unwrap().to_string())
        .collect()
}

#[sqlx::test]
async fn only_teachers_manage_topics(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let body = json!({"title": "什麼是正義？", "category": "倫理學"});

    // 教師與管理者（也算能教學）都能建立
    for cookie in [&a.teacher, &a.admin] {
        let res = send(
            &app,
            json_req("POST", "/api/topics", Some(cookie), Some(body.clone())),
        )
        .await;
        assert_eq!(res.status(), StatusCode::CREATED);
        assert_eq!(json_body(res).await["is_active"], true);
    }
    let id = create_topic(&app, &a.teacher, "T", "").await;

    for (cookie, expected) in [
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        (None, StatusCode::UNAUTHORIZED),
    ] {
        let res = send(
            &app,
            json_req("POST", "/api/topics", cookie, Some(body.clone())),
        )
        .await;
        assert_eq!(res.status(), expected);
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
        assert_eq!(res.status(), expected);
        let res = send(&app, get("/api/topics", cookie)).await;
        assert_eq!(res.status(), expected);
    }

    let res = send(
        &app,
        json_req(
            "POST",
            "/api/topics",
            Some(&a.teacher),
            Some(json!({"title": "  "})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn any_teacher_can_edit_any_topic(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    // 第二位教師（助教，權限相同）
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
    let id = create_topic(&app, &a.teacher, "A", "").await;

    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/topics/{id}"),
            Some(&ta),
            Some(json!({"title": "A2", "description": "新說明"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    assert_eq!(body["title"], "A2");
    assert_eq!(body["description"], "新說明");
    let all = json_body(send(&app, get("/api/topics", Some(&a.teacher))).await).await;
    assert_eq!(all[0]["title"], "A2");
}

#[sqlx::test]
async fn deactivated_topic_disappears_for_students(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let id = create_topic(&app, &a.teacher, "T1", "").await;

    let avail = json_body(send(&app, get("/api/available", Some(&a.student))).await).await;
    assert_eq!(ids(&avail), vec![id.clone()]);

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
    assert_eq!(res.status(), StatusCode::OK);
    let avail = json_body(send(&app, get("/api/available", Some(&a.student))).await).await;
    assert!(ids(&avail).is_empty());
    // 教師仍看得到停用的題目
    let all = json_body(send(&app, get("/api/topics", Some(&a.teacher))).await).await;
    assert_eq!(all.as_array().unwrap().len(), 1);

    // 可重新啟用
    send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/topics/{id}"),
            Some(&a.teacher),
            Some(json!({"is_active": true})),
        ),
    )
    .await;
    let avail = json_body(send(&app, get("/api/available", Some(&a.student))).await).await;
    assert_eq!(ids(&avail), vec![id]);

    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/topics/{}", uuid::Uuid::new_v4()),
            Some(&a.teacher),
            Some(json!({})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn available_list_requires_enrollment(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let res = send(&app, get("/api/available", Some(&a.outsider))).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(res).await["error"]["code"], "not_enrolled");
    assert_eq!(
        send(&app, get("/api/available", None)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    // 教師與管理者不需要在名單內
    assert_eq!(
        send(&app, get("/api/available", Some(&a.teacher)))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(&app, get("/api/available", Some(&a.admin)))
            .await
            .status(),
        StatusCode::OK
    );

    // 被移出名單後立即失去存取
    send(
        &app,
        json_req(
            "DELETE",
            "/api/roster/student@example.com",
            Some(&a.admin),
            None,
        ),
    )
    .await;
    let res = send(&app, get("/api/available", Some(&a.student))).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}
