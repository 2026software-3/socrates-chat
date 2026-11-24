mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;
#[sqlx::test]
async fn staff_can_rename_a_shared_work_without_changing_its_editions_or_links(pool: PgPool) {
    let app = test_app(pool.clone());
    let a = seed_actors(&app).await;
    let response = send(
        &app,
        json_req(
            "POST",
            "/api/books",
            Some(&a.admin),
            Some(json!({"title":"原版本"})),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let book = json_body(response).await;
    let work = book["work_id"].as_str().unwrap();
    let path = format!("/api/book-works/{work}");
    let topic: uuid::Uuid =
        sqlx::query_scalar("INSERT INTO topics(title) VALUES('Synthetic') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query("INSERT INTO topic_books(topic_id,book_id) VALUES($1,$2)")
        .bind(topic)
        .bind(work.parse::<uuid::Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    for (cookie, status) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            send(
                &app,
                json_req("PATCH", &path, cookie, Some(json!({"title":"不可修改"})))
            )
            .await
            .status(),
            status
        );
    }
    for cookie in [&a.teacher, &a.admin] {
        let response = send(
            &app,
            json_req(
                "PATCH",
                &path,
                Some(cookie),
                Some(json!({"title":"  新作品名  "})),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let data = json_body(response).await;
        assert_eq!(data["title"], "新作品名");
        assert_eq!(data["editions"][0]["title"], "原版本");
    }
    for title in ["   ".into(), "字".repeat(256)] {
        assert_eq!(
            send(
                &app,
                json_req(
                    "PATCH",
                    &path,
                    Some(&a.teacher),
                    Some(json!({"title":title}))
                )
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        send(
            &app,
            json_req(
                "PATCH",
                &path,
                Some(&a.teacher),
                Some(json!({"title":"字".repeat(255)}))
            )
        )
        .await
        .status(),
        StatusCode::OK
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM topic_books WHERE topic_id=$1 AND book_id=$2")
            .bind(topic)
            .bind(work.parse::<uuid::Uuid>().unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
    assert_eq!(
        send(
            &app,
            json_req(
                "PATCH",
                &format!("/api/book-works/{}", uuid::Uuid::new_v4()),
                Some(&a.teacher),
                Some(json!({"title":"Missing"}))
            )
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
}
