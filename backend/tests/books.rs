//! F-14.3 書庫 HTTP 契約與授權；全部使用合成資料。
mod common;

use axum::{Router, http::StatusCode};
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

async fn create_book(app: &Router, cookie: &str, title: &str) -> Value {
    let res = send(
        app,
        json_req(
            "POST",
            "/api/books",
            Some(cookie),
            Some(json!({"title":title})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    json_body(res).await
}

#[sqlx::test]
async fn teacher_creates_global_title_only_book_and_nonstaff_are_rejected(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    for (cookie, status) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
    ] {
        let res = send(
            &app,
            json_req(
                "POST",
                "/api/books",
                cookie,
                Some(json!({"title":"公平的練習"})),
            ),
        )
        .await;
        assert_eq!(res.status(), status);
    }
    let book = create_book(&app, &a.teacher, "  公平的練習  ").await;
    assert_eq!(book["title"], "公平的練習");
    assert!(book["authors"].is_null());
    assert!(book["is_active"].as_bool().unwrap());
    let res = send(&app, json_req("GET", "/api/books", Some(&a.admin), None)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_body(res).await[0]["id"], book["id"]);
}

#[sqlx::test]
async fn teachers_only_edit_own_books_and_admin_deletes_with_confirmation(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let book = create_book(&app, &a.teacher, "原始書名").await;
    let id = book["id"].as_str().unwrap();
    let url = format!("/api/books/{id}");
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(&a.admin),
            Some(json!({"email":"book-teacher@example.com"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let other = login_as(&app, "book-teacher", "book-teacher@example.com").await;
    assert_eq!(
        send(&app, json_req("GET", &url, Some(&other), None))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(
            &app,
            json_req(
                "PUT",
                &url,
                Some(&other),
                Some(json!({"title":"不允許覆寫"}))
            )
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let res = send(
        &app,
        json_req(
            "PUT",
            &url,
            Some(&a.teacher),
            Some(json!({"title":"教師修改"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_body(res).await["title"], "教師修改");
    let res = send(
        &app,
        json_req(
            "PUT",
            &url,
            Some(&a.admin),
            Some(json!({"title":"管理者修改"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    for (method, path, body) in [
        ("PATCH", format!("{url}/status"), json!({"is_active":false})),
        ("DELETE", url.clone(), json!({"confirm_title":"管理者修改"})),
    ] {
        assert_eq!(
            send(&app, json_req(method, &path, Some(&a.teacher), Some(body)))
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
    }
    for active in [false, true] {
        let res = send(
            &app,
            json_req(
                "PATCH",
                &format!("{url}/status"),
                Some(&a.admin),
                Some(json!({"is_active":active})),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(json_body(res).await["is_active"], active);
    }
    let res = send(
        &app,
        json_req(
            "DELETE",
            &url,
            Some(&a.admin),
            Some(json!({"confirm_title":"錯誤名稱"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(res).await["error"]["code"],
        "confirmation_mismatch"
    );
    assert_eq!(
        send(&app, json_req("GET", &url, Some(&a.teacher), None))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(
            &app,
            json_req(
                "DELETE",
                &url,
                Some(&a.admin),
                Some(json!({"confirm_title":"管理者修改"}))
            )
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(&app, json_req("GET", &url, Some(&a.teacher), None))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
async fn book_fields_use_unicode_limits_and_validate_isbn_and_http_links(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let input = json!({"title":"𠮷".repeat(255),"authors":"作".repeat(1000),"description":"簡".repeat(5000),"recommendation_note":"薦".repeat(2000),"isbn":"0-306-40615-2","source_url":"https://example.com/book","language":"  ","publication_year":" 2025 "});
    let res = send(
        &app,
        json_req("POST", "/api/books", Some(&a.teacher), Some(input)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let book = json_body(res).await;
    assert_eq!(book["isbn"], "9780306406157");
    assert!(book["language"].is_null());
    assert_eq!(book["publication_year"], "2025");
    for (field, value, code) in [
        ("title", " ".to_string(), "invalid_book_title"),
        ("title", "𠮷".repeat(256), "invalid_book_title"),
        ("authors", "作".repeat(1001), "invalid_book_authors"),
        ("description", "簡".repeat(5001), "invalid_book_description"),
        (
            "recommendation_note",
            "薦".repeat(2001),
            "invalid_book_recommendation_note",
        ),
        ("isbn", "9780306406150".to_string(), "invalid_book_isbn"),
        (
            "isbn",
            "０３０６４０６１５２".to_string(),
            "invalid_book_isbn",
        ),
        (
            "source_url",
            "javascript:alert(1)".to_string(),
            "invalid_book_source_url",
        ),
        (
            "source_url",
            "https://".to_string(),
            "invalid_book_source_url",
        ),
        (
            "source_url",
            format!("https://example.com/{}", "x".repeat(8192)),
            "invalid_book_source_url",
        ),
    ] {
        let mut body = json!({"title":"欄位測試"});
        body[field] = json!(value);
        let res = send(
            &app,
            json_req("POST", "/api/books", Some(&a.teacher), Some(body)),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{field}");
        assert_eq!(json_body(res).await["error"]["code"], code);
    }
}

#[sqlx::test]
async fn activity_books_copy_defaults_and_remain_independent_with_authorization(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let topic = create_topic(&app, &a, "書單題目").await;
    let one = create_book(&app, &a.teacher, "預設書目").await;
    let two = create_book(&app, &a.teacher, "活動新增書目").await;
    let topic_path = format!("/api/topics/{topic}/books");
    let res = send(
        &app,
        json_req(
            "PUT",
            &topic_path,
            Some(&a.teacher),
            Some(json!({"work_ids":[one["work_id"]]})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let activity = create_activity(&app, &a, "書單活動", &topic).await;
    let activity_path = format!("/api/activities/{activity}/topics/{topic}/books");
    let copied = json_body(
        send(
            &app,
            json_req("GET", &activity_path, Some(&a.teacher), None),
        )
        .await,
    )
    .await;
    assert_eq!(copied[0]["id"], one["work_id"]);
    assert_eq!(
        send(
            &app,
            json_req(
                "PUT",
                &activity_path,
                Some(&a.teacher),
                Some(json!({"work_ids":[two["work_id"]]}))
            )
        )
        .await
        .status(),
        StatusCode::OK
    );
    let original =
        json_body(send(&app, json_req("GET", &topic_path, Some(&a.teacher), None)).await).await;
    assert_eq!(original[0]["id"], one["work_id"]);
    // Editing the same topic_ids must not reset customized activity books.
    assert_eq!(
        send(
            &app,
            json_req(
                "PATCH",
                &format!("/api/activities/{activity}"),
                Some(&a.teacher),
                Some(json!({"topic_ids":[topic]}))
            )
        )
        .await
        .status(),
        StatusCode::OK
    );
    let preserved = json_body(
        send(
            &app,
            json_req("GET", &activity_path, Some(&a.teacher), None),
        )
        .await,
    )
    .await;
    assert_eq!(preserved[0]["id"], two["work_id"]);
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/admin/teachers",
            Some(&a.admin),
            Some(json!({"email":"foreign-books@example.com"})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let foreign = login_as(&app, "foreign-books", "foreign-books@example.com").await;
    for method in ["GET", "PUT"] {
        let res = send(
            &app,
            json_req(
                method,
                &activity_path,
                Some(&foreign),
                if method == "PUT" {
                    Some(json!({"work_ids":[]}))
                } else {
                    None
                },
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
    let res = send(
        &app,
        json_req(
            "POST",
            &format!("/api/activities/{activity}/archive"),
            Some(&a.teacher),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        send(
            &app,
            json_req("GET", &activity_path, Some(&a.teacher), None)
        )
        .await
        .status(),
        StatusCode::OK
    );
    let res = send(
        &app,
        json_req(
            "PUT",
            &activity_path,
            Some(&a.teacher),
            Some(json!({"work_ids":[]})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(res).await["error"]["code"], "activity_archived");
}

#[sqlx::test]
async fn disabled_books_stay_visible_but_cannot_be_newly_linked_and_delete_cascades(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let t1 = create_topic(&app, &a, "原有書單").await;
    let t2 = create_topic(&app, &a, "另一書單").await;
    let book = create_book(&app, &a.teacher, "停用書目").await;
    let id = book["id"].as_str().unwrap();
    let p1 = format!("/api/topics/{t1}/books");
    let p2 = format!("/api/topics/{t2}/books");
    assert_eq!(
        send(
            &app,
            json_req(
                "PUT",
                &p1,
                Some(&a.teacher),
                Some(json!({"work_ids":[book["work_id"]]}))
            )
        )
        .await
        .status(),
        StatusCode::OK
    );
    let activity = create_activity(&app, &a, "受影響活動", &t1).await;
    assert_eq!(
        send(
            &app,
            json_req(
                "PATCH",
                &format!(
                    "/api/book-works/{}/status",
                    book["work_id"].as_str().unwrap()
                ),
                Some(&a.admin),
                Some(json!({"is_active":false}))
            )
        )
        .await
        .status(),
        StatusCode::OK
    );
    let listed = json_body(send(&app, json_req("GET", &p1, Some(&a.teacher), None)).await).await;
    assert_eq!(listed[0]["is_active"], false);
    assert_eq!(listed[0]["editions"][0]["is_active"], true);
    assert_eq!(
        send(
            &app,
            json_req(
                "PUT",
                &p1,
                Some(&a.teacher),
                Some(json!({"work_ids":[book["work_id"]]}))
            )
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(
            &app,
            json_req(
                "PUT",
                &p2,
                Some(&a.teacher),
                Some(json!({"work_ids":[book["work_id"]]}))
            )
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let impact = json_body(
        send(
            &app,
            json_req(
                "GET",
                &format!("/api/books/{id}/deletion-impact"),
                Some(&a.admin),
                None,
            ),
        )
        .await,
    )
    .await;
    assert_eq!(impact["topic_count"], 1);
    assert_eq!(impact["activity_topic_count"], 1);
    assert_eq!(impact["deletes_work"], true);
    assert_eq!(
        send(
            &app,
            json_req(
                "DELETE",
                &format!("/api/books/{id}"),
                Some(&a.admin),
                Some(json!({"confirm_title":"停用書目"}))
            )
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    for path in [p1, format!("/api/activities/{activity}/topics/{t1}/books")] {
        assert_eq!(
            json_body(send(&app, json_req("GET", &path, Some(&a.teacher), None)).await).await,
            json!([])
        );
    }
}

struct FakeBookSource {
    fail: bool,
    calls: std::sync::Mutex<Vec<String>>,
}
#[async_trait::async_trait]
impl socrates_chat_backend::book_sources::BookSource for FakeBookSource {
    async fn related(
        &self,
        book: &socrates_chat_backend::book_store::Book,
    ) -> Result<
        Vec<socrates_chat_backend::book_sources::RelatedEdition>,
        socrates_chat_backend::book_sources::BookSourceError,
    > {
        Ok(self
            .search(&book.title)
            .await?
            .into_iter()
            .map(|book| socrates_chat_backend::book_sources::RelatedEdition {
                work_id: "/works/OL1W".into(),
                work_title: "合成作品".into(),
                book,
            })
            .collect())
    }

    async fn search(
        &self,
        query: &str,
    ) -> Result<
        Vec<socrates_chat_backend::book_sources::BookCandidate>,
        socrates_chat_backend::book_sources::BookSourceError,
    > {
        self.calls.lock().unwrap().push(query.to_owned());
        if self.fail {
            return Err(socrates_chat_backend::book_sources::BookSourceError);
        }
        Ok(vec![socrates_chat_backend::book_sources::BookCandidate {
            source: "Google Books".into(),
            source_id: "synthetic-volume".into(),
            title: "外部合成書目".into(),
            authors: None,
            language: None,
            publication_year: None,
            isbn: None,
            source_url: Some("https://example.com/synthetic".into()),
            description: None,
        }])
    }
}

#[sqlx::test]
async fn external_search_is_staff_only_does_not_persist_and_failure_keeps_manual_creation(
    pool: PgPool,
) {
    use socrates_chat_backend::{AppState, app as make_app};
    use std::sync::Arc;
    register_fixture_pool(&pool);
    let source = Arc::new(FakeBookSource {
        fail: false,
        calls: Default::default(),
    });
    let app = make_app(
        AppState::new(
            pool.clone(),
            test_config(),
            Arc::new(FakeIdentity),
            FakeAi::with(vec![]),
        )
        .with_book_source(source.clone()),
    );
    let a = seed_actors(&app).await;
    for (cookie, status) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            send(
                &app,
                json_req("GET", "/api/books/search?q=ethics", cookie, None)
            )
            .await
            .status(),
            status
        );
    }
    assert!(source.calls.lock().unwrap().is_empty());
    assert_eq!(
        send(
            &app,
            json_req("GET", "/api/books/search?q=", Some(&a.teacher), None)
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let res = send(
        &app,
        json_req("GET", "/api/books/search?q=ethics", Some(&a.teacher), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_body(res).await[0]["source_id"], "synthetic-volume");
    assert_eq!(*source.calls.lock().unwrap(), vec!["ethics"]);
    assert_eq!(
        json_body(send(&app, json_req("GET", "/api/books", Some(&a.teacher), None)).await).await,
        json!([])
    );
    let failed = make_app(
        AppState::new(
            pool,
            test_config(),
            Arc::new(FakeIdentity),
            FakeAi::with(vec![]),
        )
        .with_book_source(Arc::new(FakeBookSource {
            fail: true,
            calls: Default::default(),
        })),
    );
    let res = send(
        &failed,
        json_req("GET", "/api/books/search?q=ethics", Some(&a.teacher), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = json_body(res).await;
    assert_eq!(body["error"]["code"], "book_source_unavailable");
    assert!(body["error"]["request_id"].is_string());
    assert!(body["error"].get("message").is_none());
    create_book(&failed, &a.teacher, "外部故障仍保留手填").await;
}

#[sqlx::test]
async fn book_list_endpoints_reject_nonstaff_and_archived_classroom_writes(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let topic = create_topic(&app, &a, "授權題目").await;
    let activity = create_activity(&app, &a, "授權活動", &topic).await;
    let book = create_book(&app, &a.teacher, "授權書目").await;
    let id = book["id"].as_str().unwrap();
    let topic_url = format!("/api/topics/{topic}/books");
    let activity_url = format!("/api/activities/{activity}/topics/{topic}/books");
    for path in [
        "/api/books".to_string(),
        "/api/book-works".to_string(),
        format!("/api/books/{id}/work-impact"),
        format!(
            "/api/book-works/{}/impact",
            book["work_id"].as_str().unwrap()
        ),
        format!("/api/books/{id}"),
        topic_url.clone(),
        activity_url.clone(),
    ] {
        for (cookie, status) in [
            (None, StatusCode::UNAUTHORIZED),
            (Some(a.student.as_str()), StatusCode::FORBIDDEN),
            (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        ] {
            assert_eq!(
                send(&app, json_req("GET", &path, cookie, None))
                    .await
                    .status(),
                status,
                "{path}"
            );
        }
    }
    for path in [topic_url, activity_url.clone()] {
        assert_eq!(
            send(
                &app,
                json_req(
                    "PUT",
                    &path,
                    Some(&a.student),
                    Some(json!({"work_ids":[book["work_id"]]}))
                )
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            send(
                &app,
                json_req(
                    "PUT",
                    &path,
                    Some(&a.teacher),
                    Some(json!({"work_ids":[book["work_id"],book["work_id"]]}))
                )
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let res = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/classrooms/{}", a.classroom),
            Some(&a.teacher),
            Some(json!({"is_archived":true})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        send(&app, json_req("GET", &activity_url, Some(&a.teacher), None))
            .await
            .status(),
        StatusCode::OK
    );
    let res = send(
        &app,
        json_req(
            "PUT",
            &activity_url,
            Some(&a.admin),
            Some(json!({"work_ids":[book["work_id"]]})),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(res).await["error"]["code"], "classroom_archived");
}

#[sqlx::test]
async fn external_source_provenance_roundtrips_independently_of_editable_url(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let manual = create_book(&app, &a.teacher, "合成手動書目").await;
    assert_eq!(manual["external_sources"], json!([]));
    let google = json!({"source":"Google Books","source_id":"synthetic_edition-1"});
    let open_library = json!({"source":"Open Library","source_id":"/books/OL123M"});
    let refs = json!([google.clone(), open_library]);
    let res = send(
        &app,
        json_req(
            "POST",
            "/api/books",
            Some(&a.teacher),
            Some(json!({
                "title":"合成外部書目", "external_sources":[google.clone(), google, refs[1]]
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let book = json_body(res).await;
    assert_eq!(book["external_sources"], refs);
    let url = format!("/api/books/{}", book["id"].as_str().unwrap());
    let res = send(
        &app,
        json_req(
            "PUT",
            &url,
            Some(&a.teacher),
            Some(json!({
                "title":"教師修正書名", "source_url":null, "external_sources":refs
            })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let res = send(&app, json_req("GET", &url, Some(&a.teacher), None)).await;
    let saved = json_body(res).await;
    assert_eq!(saved["title"], "教師修正書名");
    assert!(saved["source_url"].is_null());
    assert_eq!(saved["external_sources"], refs);
    let res = send(&app, json_req("GET", "/api/openapi.json", None, None)).await;
    let schema = json_body(res).await;
    assert!(
        schema["components"]["schemas"]["BookSourceRef"]["properties"]["source_id"].is_object()
    );
    assert_eq!(
        schema["components"]["schemas"]["BookData"]["properties"]["external_sources"]["type"],
        "array"
    );
}

#[sqlx::test]
async fn external_source_provenance_rejects_unknown_providers_and_invalid_ids(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    for (source, id) in [
        ("unknown", "id"),
        ("Google Books", ""),
        ("Google Books", "../unsafe"),
        ("Open Library", "/books/OLM"),
        ("Open Library", "/works/OL123W"),
        ("Open Library", "/books/OL字M"),
    ] {
        let res = send(
            &app,
            json_req(
                "POST",
                "/api/books",
                Some(&a.teacher),
                Some(json!({
                    "title":"合成書目", "external_sources":[{"source":source,"source_id":id}]
                })),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{source}: {id}");
        assert_eq!(
            json_body(res).await["error"]["code"],
            "invalid_book_external_sources"
        );
    }
}

#[sqlx::test]
async fn work_grouping_redirects_lists_without_granting_metadata_ownership(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let source = create_book(&app, &a.admin, "來源作品").await;
    let target = create_book(&app, &a.admin, "保留作品").await;
    let source_work = source["work_id"].as_str().unwrap();
    let target_work = target["work_id"].as_str().unwrap();
    let topic = create_topic(&app, &a, "合併書單").await;
    let list_url = format!("/api/topics/{topic}/books");
    assert_eq!(
        send(
            &app,
            json_req(
                "PUT",
                &list_url,
                Some(&a.teacher),
                Some(json!({"work_ids":[source_work,target_work]}))
            )
        )
        .await
        .status(),
        StatusCode::OK
    );
    let activity = create_activity(&app, &a, "合併活動", &topic).await;
    let merge_url = format!("/api/book-works/{source_work}/merge");
    let request = json!({"target_work_id":target_work,"confirm_title":"來源作品"});
    for (cookie, status) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(a.student.as_str()), StatusCode::FORBIDDEN),
        (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            send(
                &app,
                json_req("POST", &merge_url, cookie, Some(request.clone()))
            )
            .await
            .status(),
            status
        );
    }
    let impact = json_body(
        send(
            &app,
            json_req(
                "GET",
                &format!("/api/book-works/{source_work}/impact"),
                Some(&a.teacher),
                None,
            ),
        )
        .await,
    )
    .await;
    assert_eq!(
        impact,
        json!({"edition_count":1,"topic_count":1,"activity_topic_count":1})
    );
    assert_eq!(
        send(
            &app,
            json_req(
                "POST",
                &merge_url,
                Some(&a.teacher),
                Some(json!({"target_work_id":target_work,"confirm_title":"wrong"}))
            )
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(
            &app,
            json_req(
                "PATCH",
                &format!("/api/book-works/{target_work}/status"),
                Some(&a.admin),
                Some(json!({"is_active":false}))
            )
        )
        .await
        .status(),
        StatusCode::OK
    );
    let merged = send(
        &app,
        json_req("POST", &merge_url, Some(&a.teacher), Some(request)),
    )
    .await;
    assert_eq!(merged.status(), StatusCode::OK);
    let merged = json_body(merged).await;
    assert_eq!(merged["editions"].as_array().unwrap().len(), 2);
    assert_eq!(merged["is_active"], false);
    assert!(
        merged["editions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|edition| edition["is_active"] == true)
    );
    for url in [
        list_url,
        format!("/api/activities/{activity}/topics/{topic}/books"),
    ] {
        let rows = json_body(send(&app, json_req("GET", &url, Some(&a.teacher), None)).await).await;
        assert_eq!(rows.as_array().unwrap().len(), 1);
        assert_eq!(rows[0]["id"], target_work);
    }
    assert_eq!(
        send(
            &app,
            json_req(
                "PUT",
                &format!("/api/books/{}", source["id"].as_str().unwrap()),
                Some(&a.teacher),
                Some(json!({"title":"禁止修改"}))
            )
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(
            &app,
            json_req(
                "GET",
                &format!("/api/book-works/{source_work}/impact"),
                Some(&a.teacher),
                None
            )
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
async fn moving_last_edition_preserves_source_work_and_lists_and_admin_controls_lifecycle(
    pool: PgPool,
) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let source = create_book(&app, &a.admin, "移動版本").await;
    let target = create_book(&app, &a.admin, "目標作品").await;
    let source_work = source["work_id"].as_str().unwrap();
    let target_work = target["work_id"].as_str().unwrap();
    let edition = source["id"].as_str().unwrap();
    let topic = create_topic(&app, &a, "移動書單").await;
    let list_url = format!("/api/topics/{topic}/books");
    assert_eq!(
        send(
            &app,
            json_req(
                "PUT",
                &list_url,
                Some(&a.teacher),
                Some(json!({"work_ids":[source_work]}))
            )
        )
        .await
        .status(),
        StatusCode::OK
    );
    for body in [
        json!({}),
        json!({"work_id":target_work,"confirm_title":"錯誤版本"}),
    ] {
        assert_eq!(
            send(
                &app,
                json_req(
                    "PUT",
                    &format!("/api/books/{edition}/work"),
                    Some(&a.teacher),
                    Some(body)
                )
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let unchanged = json_body(
        send(
            &app,
            json_req(
                "GET",
                &format!("/api/books/{edition}"),
                Some(&a.teacher),
                None,
            ),
        )
        .await,
    )
    .await;
    assert_eq!(unchanged["work_id"], source_work);
    let moved = send(
        &app,
        json_req(
            "PUT",
            &format!("/api/books/{edition}/work"),
            Some(&a.teacher),
            Some(json!({"work_id":target_work,"confirm_title":"移動版本"})),
        ),
    )
    .await;
    assert_eq!(moved.status(), StatusCode::OK);
    assert_eq!(json_body(moved).await["work_id"], target_work);
    let rows =
        json_body(send(&app, json_req("GET", &list_url, Some(&a.teacher), None)).await).await;
    assert_eq!(rows[0]["id"], source_work);
    assert_eq!(rows[0]["editions"], json!([]));
    for (method, suffix, body) in [
        ("PATCH", "/status", json!({"is_active":false})),
        ("DELETE", "", json!({"confirm_title":"目標作品"})),
    ] {
        assert_eq!(
            send(
                &app,
                json_req(
                    method,
                    &format!("/api/book-works/{target_work}{suffix}"),
                    Some(&a.teacher),
                    Some(body)
                )
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }
    for body in [json!({}), json!({"confirm_title":"錯誤作品"})] {
        assert_eq!(
            send(
                &app,
                json_req(
                    "DELETE",
                    &format!("/api/book-works/{target_work}"),
                    Some(&a.admin),
                    Some(body)
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
                "GET",
                &format!("/api/books/{edition}"),
                Some(&a.teacher),
                None
            )
        )
        .await
        .status(),
        StatusCode::OK
    );
    let status = send(
        &app,
        json_req(
            "PATCH",
            &format!("/api/book-works/{target_work}/status"),
            Some(&a.admin),
            Some(json!({"is_active":false})),
        ),
    )
    .await;
    assert_eq!(status.status(), StatusCode::OK);
    let work = json_body(status).await;
    assert_eq!(work["is_active"], false);
    assert!(
        work["editions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|edition| edition["is_active"] == true)
    );
    assert_eq!(
        send(
            &app,
            json_req(
                "DELETE",
                &format!("/api/book-works/{target_work}"),
                Some(&a.admin),
                Some(json!({"confirm_title":"目標作品"}))
            )
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            &app,
            json_req(
                "GET",
                &format!("/api/books/{edition}"),
                Some(&a.teacher),
                None
            )
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
async fn work_operations_reject_nonstaff_before_reading_or_mutating_catalog(pool: PgPool) {
    let app = test_app(pool);
    let a = seed_actors(&app).await;
    let id = "00000000-0000-0000-0000-000000000000";
    for (method, path) in [
        ("GET", "/api/book-works".to_owned()),
        ("GET", format!("/api/book-works/{id}/impact")),
        ("POST", format!("/api/book-works/{id}/merge")),
        ("PATCH", format!("/api/book-works/{id}/status")),
        ("DELETE", format!("/api/book-works/{id}")),
        ("GET", format!("/api/books/{id}/work-impact")),
        ("PUT", format!("/api/books/{id}/work")),
    ] {
        for (cookie, expected) in [
            (None, StatusCode::UNAUTHORIZED),
            (Some(a.student.as_str()), StatusCode::FORBIDDEN),
            (Some(a.outsider.as_str()), StatusCode::FORBIDDEN),
        ] {
            assert_eq!(
                send(&app, json_req(method, &path, cookie, Some(json!({}))))
                    .await
                    .status(),
                expected,
                "{method} {path}"
            );
        }
    }
}

#[sqlx::test]
async fn related_edition_endpoints_require_staff_and_import_atomically(pool: PgPool) {
    let app = test_app(pool.clone());
    let actors = seed_actors(&app).await;
    let book = create_book(&app, &actors.teacher, "合成作品").await;
    let id = book["id"].as_str().unwrap();
    let lookup = format!("/api/books/{id}/editions");
    let add = format!("/api/book-works/{id}/editions");
    for (cookie, status) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(actors.student.as_str()), StatusCode::FORBIDDEN),
        (Some(actors.outsider.as_str()), StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            send(&app, json_req("GET", &lookup, cookie, None))
                .await
                .status(),
            status
        );
        assert_eq!(
            send(
                &app,
                json_req(
                    "POST",
                    &add,
                    cookie,
                    Some(
                        json!({"confirm_title":"合成作品","books":[{"title":"Synthetic edition"}]})
                    )
                )
            )
            .await
            .status(),
            status
        );
    }
    let body = json!({"confirm_title":"合成作品","books":[{"title":"Synthetic edition","language":"en","external_sources":[{"source":"Open Library","source_id":"/books/OL123M"}]}]});
    for _ in 0..2 {
        assert_eq!(
            send(
                &app,
                json_req("POST", &add, Some(&actors.teacher), Some(body.clone()))
            )
            .await
            .status(),
            StatusCode::OK
        );
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM books")
            .fetch_one(&pool)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM book_works")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        send(
            &app,
            json_req(
                "POST",
                &add,
                Some(&actors.teacher),
                Some(json!({"confirm_title":"wrong","books":[{"title":"Do not add"}]}))
            )
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(send(&app,json_req("POST",&add,Some(&actors.teacher),Some(json!({"confirm_title":"合成作品","books":[{"title":"Do not add"},{"title":" "}]})))).await.status(),StatusCode::BAD_REQUEST);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM books")
            .fetch_one(&pool)
            .await
            .unwrap(),
        2
    );
}

#[sqlx::test]
async fn related_lookup_deduplicates_catalog_and_failure_preserves_saved_book(pool: PgPool) {
    use socrates_chat_backend::{AppState, app as make_app};
    use std::sync::Arc;
    register_fixture_pool(&pool);
    let build = |fail| {
        make_app(
            AppState::new(
                pool.clone(),
                test_config(),
                Arc::new(FakeIdentity),
                FakeAi::with(vec![]),
            )
            .with_book_source(Arc::new(FakeBookSource {
                fail,
                calls: Default::default(),
            })),
        )
    };
    let app = build(false);
    let a = seed_actors(&app).await;
    let seed = create_book(&app, &a.teacher, "合成原書").await;
    let url = format!("/api/books/{}/editions", seed["id"].as_str().unwrap());
    let response = send(&app, json_req("GET", &url, Some(&a.teacher), None)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await.as_array().unwrap().len(), 1);
    let known = send(&app, json_req("POST", "/api/books", Some(&a.teacher), Some(json!({"title":"其他作品已有版本","external_sources":[{"source":"Google Books","source_id":"synthetic-volume"}]})))).await;
    assert_eq!(known.status(), StatusCode::CREATED);
    let known = json_body(known).await;
    assert_eq!(
        json_body(send(&app, json_req("GET", &url, Some(&a.admin), None)).await).await,
        json!([])
    );
    let target = format!(
        "/api/book-works/{}/editions",
        seed["work_id"].as_str().unwrap()
    );
    let response = send(&app, json_req("POST", &target, Some(&a.teacher), Some(json!({"confirm_title":"合成原書","books":[{"title":"重複版本","external_sources":[{"source":"Google Books","source_id":"synthetic-volume"}]}]})))).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["editions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let original = format!("/api/books/{}", known["id"].as_str().unwrap());
    assert_eq!(
        json_body(send(&app, json_req("GET", &original, Some(&a.teacher), None)).await).await["work_id"],
        known["work_id"]
    );
    let failed = build(true);
    let response = send(&failed, json_req("GET", &url, Some(&a.teacher), None)).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        json_body(response).await["error"]["code"],
        "book_source_unavailable"
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM books")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
}
