//! Keep customized activity reading lists through the actual activity PATCH endpoint.
mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test]
async fn activity_patch_reorders_existing_lists_and_copies_only_new_topics(pool: PgPool) {
    let app = test_app(pool);
    let actors = seed_actors(&app).await;
    let first = create_topic(&app, &actors, "第一題").await;
    let second = create_topic(&app, &actors, "第二題").await;
    let third = create_topic(&app, &actors, "新增題").await;
    let mut ids = Vec::new();
    for title in ["原預設", "活動專用"] {
        let res = send(
            &app,
            json_req(
                "POST",
                "/api/books",
                Some(&actors.teacher),
                Some(json!({"title":title})),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::CREATED);
        ids.push(json_body(res).await["work_id"].as_str().unwrap().to_owned());
    }
    for topic in [&first, &third] {
        let res = send(
            &app,
            json_req(
                "PUT",
                &format!("/api/topics/{topic}/books"),
                Some(&actors.teacher),
                Some(json!({"work_ids":[ids[0]]})),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
    }
    let res = send(&app, json_req("POST", "/api/activities", Some(&actors.teacher), Some(json!({"title":"合成活動","classroom_id":actors.classroom,"topic_ids":[first,second]})))).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let activity = json_body(res).await["id"].as_str().unwrap().to_owned();
    let res = send(
        &app,
        json_req(
            "PUT",
            &format!("/api/activities/{activity}/topics/{first}/books"),
            Some(&actors.teacher),
            Some(json!({"work_ids":[ids[1]]})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/activities/{activity}"),
            Some(&actors.teacher),
            Some(json!({"topic_ids":[second,first,third]})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    for (topic, expected) in [(&first, &ids[1]), (&third, &ids[0])] {
        let res = send(
            &app,
            get(
                &format!("/api/activities/{activity}/topics/{topic}/books"),
                Some(&actors.teacher),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(json_body(res).await[0]["id"].as_str().unwrap(), expected);
    }
}
