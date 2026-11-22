//! F-14.1 recommendation API, using synthetic conversations and bibliographic data.
mod common;

use axum::{Router, http::StatusCode};
use common::*;
use serde_json::{Value, json};
use sqlx::PgPool;

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};
use uuid::Uuid;

async fn recommendation(
    app: &Router,
    method: &str,
    conversation: &str,
    cookie: Option<&str>,
) -> (StatusCode, Value) {
    let suffix = if method == "POST" { "/retry" } else { "" };
    let response = send(
        app,
        json_req(
            method,
            &format!("/api/conversations/{conversation}/recommendations{suffix}"),
            cookie,
            None,
        ),
    )
    .await;
    (response.status(), json_body(response).await)
}

#[sqlx::test]
async fn owner_sees_waiting_for_summary_and_staff_cannot_read(pool: PgPool) {
    let app = test_app(pool);
    let actors = seed_actors(&app).await;
    let conversation = start_conversation(&app, &actors).await;

    let (status, body) = recommendation(&app, "GET", &conversation, Some(&actors.student)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({
            "status": "waiting_for_summary",
            "recommendations": [],
            "teacher_reference_list": [],
            "error_code": null,
            "retry_at": null,
            "can_retry": false
        })
    );
    assert_eq!(
        recommendation(&app, "GET", &conversation, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    for cookie in [&actors.teacher, &actors.admin] {
        assert_eq!(
            recommendation(&app, "GET", &conversation, Some(cookie))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    let (status, body) = recommendation(&app, "POST", &conversation, Some(&actors.student)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "recommendation_not_retryable");
    assert!(body["error"]["request_id"].is_string());

    let doc = json_body(send(&app, json_req("GET", "/api/openapi.json", None, None)).await).await;
    let schemas = &doc["components"]["schemas"];
    assert_eq!(
        schemas["RecommendationView"]["properties"]["status"]["$ref"],
        "#/components/schemas/RecommendationStatus"
    );
    assert_eq!(
        schemas["RecommendationStatus"]["enum"],
        json!([
            "waiting_for_summary",
            "pending",
            "ready",
            "no_results",
            "failed",
            "interrupted"
        ])
    );
    assert!(
        schemas["Status"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("healthy"))
    );
}

use async_trait::async_trait;
use futures_util::{StreamExt, stream};
use socrates_chat_backend::{
    AppState,
    ai::{AiError, AiProvider, AiRequest, AiStream, StructuredCompletion, StructuredRequest},
    app,
    book_sources::{
        BookCandidate, BookSource, BookSourceError, RecommendationEdition, RecommendationSource,
    },
    book_store::{self, Book, BookData},
};
const COVERED: &str = r#"{"picks":[{"id":"0","reason":"可延伸對公平的疑問。","question":"哪種分配理由可以說服你？"}],"proposals":[]}"#;
const NEEDS_SOURCE: &str =
    r#"{"picks":[],"proposals":[{"title":"正義與社會","author":"合成作者"}]}"#;
const REJECT_ALL: &str = r#"{"picks":[],"proposals":[]}"#;

#[derive(Default)]
struct ScriptedAi {
    choices: Mutex<VecDeque<&'static str>>,
    requests: Mutex<Vec<Value>>,
    wait_for: Option<Arc<tokio::sync::Notify>>,
}

#[async_trait]
impl AiProvider for ScriptedAi {
    async fn stream_chat(&self, _req: AiRequest) -> Result<AiStream, AiError> {
        Ok(stream::iter([Ok(
            r#"{"stance":"無明確立場","reasons":"追問什麼是公平"}"#.into()
        )])
        .boxed())
    }

    async fn complete_json(&self, req: StructuredRequest) -> Result<StructuredCompletion, AiError> {
        if req.name != "book_recommendations" {
            return Err(AiError::unclassified());
        }
        self.requests.lock().unwrap().push(req.input);
        if let Some(wait) = &self.wait_for {
            wait.notified().await;
        }
        let content = self
            .choices
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(AiError::unclassified)?;
        Ok(StructuredCompletion {
            content: content.into(),
            model: "synthetic".into(),
        })
    }
}

#[derive(Default)]
struct Sources {
    google: Option<Vec<BookCandidate>>,
    ol: Option<Vec<BookCandidate>>,
    calls: Mutex<Vec<(&'static str, String)>>,
}

#[async_trait]
impl BookSource for Sources {
    async fn search(&self, _query: &str) -> Result<Vec<BookCandidate>, BookSourceError> {
        panic!("recommendations must use independent provider lookups")
    }

    async fn search_recommendations(
        &self,
        provider: RecommendationSource,
        concept: &str,
    ) -> Result<Vec<RecommendationEdition>, BookSourceError> {
        let (label, books) = match provider {
            RecommendationSource::GoogleBooks => ("Google Books", &self.google),
            RecommendationSource::OpenLibrary => ("Open Library", &self.ol),
        };
        self.calls.lock().unwrap().push((label, concept.into()));
        let books = books.as_ref().ok_or(BookSourceError)?;
        Ok(books
            .iter()
            .cloned()
            .map(|book| RecommendationEdition {
                work_id: None,
                book,
            })
            .collect())
    }
}

fn external(source: &str, title: &str, language: &str) -> BookCandidate {
    BookCandidate {
        source: source.into(),
        source_id: "synthetic-id".into(),
        title: title.into(),
        authors: Some("合成作者".into()),
        language: Some(language.into()),
        publication_year: None,
        isbn: None,
        source_url: None,
        description: None,
    }
}

async fn fixture(
    pool: &PgPool,
    ai: Arc<ScriptedAi>,
    sources: Arc<Sources>,
) -> (Router, Actors, String) {
    register_fixture_pool(pool);
    let app = app(
        AppState::new(pool.clone(), test_config(), Arc::new(FakeIdentity), ai)
            .with_book_source(sources),
    );
    let actors = seed_actors(&app).await;
    let conversation = start_conversation(&app, &actors).await;
    seed_student_message(
        pool,
        &conversation,
        "什麼是公平？不要把 synth-person@example.test 傳給外部書庫。",
    )
    .await;
    (app, actors, conversation)
}

async fn finish_dialogue(app: &Router, actors: &Actors, conversation: &str) {
    let res = send(
        app,
        json_req(
            "POST",
            &format!("/api/conversations/{conversation}/end"),
            Some(&actors.student),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    assert_eq!(
        settled_summary(app, &actors.student, conversation).await["status"],
        "ready"
    );
}

async fn settled(app: &Router, actors: &Actors, conversation: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let (status, body) =
                recommendation(app, "GET", conversation, Some(&actors.student)).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            if body["status"] != "pending" {
                return body;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("recommendation must settle")
}

async fn seed_book(pool: &PgPool, work: Option<Uuid>, title: &str, language: Option<&str>) -> Book {
    let data = BookData {
        title: title.into(),
        language: language.map(str::to_owned),
        ..BookData::default()
    };
    if let Some(work) = work {
        let teacher =
            sqlx::query_scalar("SELECT id FROM users WHERE email = 'teacher@example.com'")
                .fetch_one(pool)
                .await
                .unwrap();
        book_store::create_in_work(pool, teacher, &data, work)
            .await
            .unwrap()
    } else {
        book_store::create(pool, None, &data).await.unwrap()
    }
}

async fn put_on_activity_list(pool: &PgPool, conversation: &str, work: Uuid) {
    sqlx::query(
        "INSERT INTO activity_topic_books (activity_id, topic_id, book_id)
                SELECT activity_id, topic_id, $2 FROM conversations WHERE id = $1",
    )
    .bind(Uuid::parse_str(conversation).unwrap())
    .bind(work)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test]
async fn summary_stays_available_while_books_run_and_snapshot_survives_catalog_changes(
    pool: PgPool,
) {
    let gate = Arc::new(tokio::sync::Notify::new());
    let ai = Arc::new(ScriptedAi {
        choices: Mutex::new([COVERED].into()),
        wait_for: Some(gate.clone()),
        ..ScriptedAi::default()
    });
    let sources = Arc::new(Sources::default());
    let (app, actors, conv) = fixture(&pool, ai.clone(), sources.clone()).await;
    let book = seed_book(&pool, None, "公平的原則", Some("zh")).await;
    seed_book(
        &pool,
        Some(book.work_id),
        "公平的原則（新版）",
        Some("zh-Hant-TW"),
    )
    .await;
    seed_book(&pool, Some(book.work_id), "公平的原则", Some("zh-Hans")).await;
    seed_book(&pool, Some(book.work_id), "語言未提供", None).await;
    put_on_activity_list(&pool, &conv, book.work_id).await;

    finish_dialogue(&app, &actors, &conv).await;
    assert_eq!(
        recommendation(&app, "GET", &conv, Some(&actors.student))
            .await
            .1["status"],
        "pending"
    );
    gate.notify_one();
    let saved = settled(&app, &actors, &conv).await;
    assert_eq!(saved["status"], "ready");
    let result = &saved["recommendations"][0];
    assert_eq!(result["source"], "teacher_list");
    assert_eq!(result["teacher_recommended"], true);
    assert_eq!(result["editions"].as_array().unwrap().len(), 2);
    assert_eq!(result["editions"][0]["language"], "zh");
    assert!(result["editions"][0]["authors"].is_null());
    assert!(sources.calls.lock().unwrap().is_empty());
    {
        let requests = ai.requests.lock().unwrap();
        let input = requests[0].to_string();
        assert!(input.contains("[EMAIL]"));
        assert!(!input.contains("synth-person"));
        assert!(!input.contains("student@example.com"));
        assert_eq!(requests.len(), 1);
    }

    sqlx::query("DELETE FROM book_works WHERE id = $1")
        .bind(book.work_id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        socrates_chat_backend::book_recommendations::recover_orphaned(&pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(settled(&app, &actors, &conv).await, saved);
    assert_eq!(
        recommendation(&app, "POST", &conv, Some(&actors.student))
            .await
            .0,
        StatusCode::CONFLICT
    );

    let other = add_student(&app, &actors, "sub-other", "other@example.com").await;
    assert_eq!(
        recommendation(&app, "GET", &conv, Some(&other)).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        recommendation(&app, "GET", &conv, Some(&actors.outsider))
            .await
            .0,
        StatusCode::FORBIDDEN
    );

    sqlx::query("DELETE FROM conversations WHERE id = $1")
        .bind(Uuid::parse_str(&conv).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM book_recommendations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[sqlx::test]
async fn google_usable_stops_fallback_even_when_only_one_book(pool: PgPool) {
    let sources = Arc::new(Sources {
        google: Some(vec![external("Google Books", "正義與社會", "zh-TW")]),
        ol: None,
        ..Sources::default()
    });
    let ai = Arc::new(ScriptedAi {
        choices: Mutex::new([NEEDS_SOURCE, COVERED].into()),
        ..ScriptedAi::default()
    });
    let (app, actors, conv) = fixture(&pool, ai, sources.clone()).await;
    finish_dialogue(&app, &actors, &conv).await;
    let saved = settled(&app, &actors, &conv).await;
    assert_eq!(saved["status"], "ready");
    assert_eq!(saved["recommendations"][0]["source"], "google_books");
    assert_eq!(
        saved["recommendations"][0]["editions"][0]["title"],
        "正義與社會"
    );
    assert_eq!(saved["recommendations"][0]["teacher_recommended"], false);
    assert_eq!(
        *sources.calls.lock().unwrap(),
        [("Google Books", "\"正義與社會\" \"合成作者\"".into())]
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM books")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "external results are private snapshots only");
}

#[sqlx::test]
async fn google_rejected_for_relevance_uses_open_library(pool: PgPool) {
    let sources = Arc::new(Sources {
        google: Some(vec![external("Google Books", "正義與社會", "zh")]),
        ol: Some(vec![external("Open Library", "正義與社會", "zh-Hant")]),
        ..Sources::default()
    });
    let ai = Arc::new(ScriptedAi {
        choices: Mutex::new([NEEDS_SOURCE, REJECT_ALL, COVERED].into()),
        ..ScriptedAi::default()
    });
    let (app, actors, conv) = fixture(&pool, ai, sources.clone()).await;
    finish_dialogue(&app, &actors, &conv).await;
    let saved = settled(&app, &actors, &conv).await;
    assert_eq!(saved["status"], "ready");
    assert_eq!(saved["recommendations"][0]["source"], "open_library");
    assert_eq!(
        *sources.calls.lock().unwrap(),
        [
            ("Google Books", "\"正義與社會\" \"合成作者\"".into()),
            ("Open Library", "\"正義與社會\" \"合成作者\"".into())
        ]
    );
}

#[sqlx::test]
async fn open_library_generic_chinese_codes_are_eligible_without_relabeling(pool: PgPool) {
    let sources = Arc::new(Sources {
        google: Some(vec![external("Google Books", "正义与社会", "zh-CN")]),
        ol: Some(
            ["chi", "zho"]
                .into_iter()
                .map(|language| BookCandidate {
                    source_id: format!("synthetic-{language}"),
                    ..external("Open Library", "正義與社會", language)
                })
                .collect(),
        ),
        ..Sources::default()
    });
    let picks = r#"{"picks":[{"id":"0","reason":"探討公平","question":"如何理解公平？"},{"id":"1","reason":"探討公平的應用","question":"公平原則如何適用？"}],"proposals":[]}"#;
    let ai = Arc::new(ScriptedAi {
        choices: Mutex::new([NEEDS_SOURCE, picks].into()),
        ..ScriptedAi::default()
    });
    let (app, actors, conv) = fixture(&pool, ai, sources).await;
    finish_dialogue(&app, &actors, &conv).await;
    let saved = settled(&app, &actors, &conv).await;
    assert_eq!(saved["status"], "ready");
    let books = saved["recommendations"].as_array().unwrap();
    assert_eq!(books.len(), 2);
    for (book, language) in books.iter().zip(["chi", "zho"]) {
        assert_eq!(book["source"], "open_library");
        assert_eq!(book["editions"][0]["language"], language);
    }
}

#[sqlx::test]
async fn external_failure_has_complete_teacher_reference_list_and_retry_cooldown(pool: PgPool) {
    let sources = Arc::new(Sources::default());
    let ai = Arc::new(ScriptedAi {
        choices: Mutex::new([NEEDS_SOURCE, COVERED].into()),
        ..ScriptedAi::default()
    });
    let (app, actors, conv) = fixture(&pool, ai.clone(), sources.clone()).await;
    let book = seed_book(&pool, None, "正義與選擇", Some("zh")).await;
    put_on_activity_list(&pool, &conv, book.work_id).await;
    for i in 0..6 {
        let book = seed_book(&pool, None, &format!("Reference {i}"), Some("es")).await;
        put_on_activity_list(&pool, &conv, book.work_id).await;
    }
    finish_dialogue(&app, &actors, &conv).await;
    let failed = settled(&app, &actors, &conv).await;
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["error_code"], "book_source_unavailable");
    let list = failed["teacher_reference_list"].as_array().unwrap();
    assert_eq!(list.len(), 7);
    assert!(
        list.iter()
            .all(|work| work.get("reason").is_none() && work.get("question").is_none())
    );
    assert_eq!(
        *sources.calls.lock().unwrap(),
        [("Google Books", "\"正義與社會\" \"合成作者\"".into())]
    );
    assert_eq!(
        recommendation(&app, "POST", &conv, Some(&actors.student))
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    sqlx::query("UPDATE book_recommendations SET started_at = now() - interval '31 seconds'")
        .execute(&pool)
        .await
        .unwrap();

    let (left, right) = tokio::join!(
        recommendation(&app, "POST", &conv, Some(&actors.student)),
        recommendation(&app, "POST", &conv, Some(&actors.student)),
    );
    assert!([left.0, right.0].contains(&StatusCode::ACCEPTED));
    assert!([left.0, right.0].contains(&StatusCode::CONFLICT));
    assert_eq!(settled(&app, &actors, &conv).await["status"], "ready");
    assert_eq!(ai.requests.lock().unwrap().len(), 2);
}

#[sqlx::test]
async fn no_results_is_saved_and_not_retryable(pool: PgPool) {
    let sources = Arc::new(Sources {
        google: Some(Vec::new()),
        ol: Some(Vec::new()),
        ..Sources::default()
    });
    let ai = Arc::new(ScriptedAi {
        choices: Mutex::new([NEEDS_SOURCE].into()),
        ..ScriptedAi::default()
    });
    let (app, actors, conv) = fixture(&pool, ai, sources.clone()).await;
    finish_dialogue(&app, &actors, &conv).await;
    let result = settled(&app, &actors, &conv).await;
    assert_eq!(result["status"], "no_results");
    assert_eq!(result["can_retry"], false);
    assert_eq!(
        recommendation(&app, "POST", &conv, Some(&actors.student))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(settled(&app, &actors, &conv).await, result);
    assert_eq!(sources.calls.lock().unwrap().len(), 2);
}

#[sqlx::test]
async fn archived_then_restored_activity_still_discards_the_running_job(pool: PgPool) {
    let gate = Arc::new(tokio::sync::Notify::new());
    let ai = Arc::new(ScriptedAi {
        choices: Mutex::new([COVERED].into()),
        wait_for: Some(gate.clone()),
        ..ScriptedAi::default()
    });
    let (app, actors, conv) = fixture(&pool, ai, Arc::default()).await;
    let book = seed_book(&pool, None, "正義與選擇", Some("zh")).await;
    put_on_activity_list(&pool, &conv, book.work_id).await;
    finish_dialogue(&app, &actors, &conv).await;
    let id = Uuid::parse_str(&conv).unwrap();
    for archived in [true, false] {
        sqlx::query("UPDATE activities SET is_archived = $2 WHERE id = (SELECT activity_id FROM conversations WHERE id = $1)")
            .bind(id).bind(archived).execute(&pool).await.unwrap();
        let body = settled(&app, &actors, &conv).await;
        assert_eq!(body["status"], "interrupted");
        assert_eq!(body["can_retry"], !archived);
    }
    gate.notify_one();
    assert_eq!(settled(&app, &actors, &conv).await["status"], "interrupted");
}

#[sqlx::test]
async fn restart_marks_pending_as_interrupted_without_automatically_running_again(pool: PgPool) {
    let ai = Arc::new(ScriptedAi {
        wait_for: Some(Arc::new(tokio::sync::Notify::new())),
        ..ScriptedAi::default()
    });
    let (app, actors, conv) = fixture(&pool, ai, Arc::default()).await;
    finish_dialogue(&app, &actors, &conv).await;
    assert_eq!(
        socrates_chat_backend::book_recommendations::recover_orphaned(&pool)
            .await
            .unwrap(),
        1
    );
    let result = settled(&app, &actors, &conv).await;
    assert_eq!(result["status"], "interrupted");
    assert_eq!(result["error_code"], "recommendation_interrupted");
    assert!(result["retry_at"].is_string());
}

#[sqlx::test]
async fn all_model_ids_invalid_is_a_failure_not_fallback(pool: PgPool) {
    let ai = Arc::new(ScriptedAi {
        choices: Mutex::new([r#"{"picks":[{"id":"unknown","reason":"合成理由","question":"合成問題？"}],"proposals":[{"title":"正義與社會","author":"合成作者"}]}"#].into()),
        ..ScriptedAi::default()
    });
    let sources = Arc::new(Sources::default());
    let (app, actors, conv) = fixture(&pool, ai, sources.clone()).await;
    finish_dialogue(&app, &actors, &conv).await;
    let result = settled(&app, &actors, &conv).await;
    assert_eq!(result["status"], "failed");
    assert_eq!(result["error_code"], "recommendation_invalid_output");
    assert!(sources.calls.lock().unwrap().is_empty());
}

#[sqlx::test]
async fn activity_selection_uses_only_the_completed_topics_teacher_list(pool: PgPool) {
    let ai = Arc::new(ScriptedAi {
        choices: Mutex::new([r#"{"picks":[{"id":"invalid","reason":"忽略","question":"忽略？"},{"id":"0","reason":"延伸原問題。","question":"什麼能改變你的判斷？"}],"proposals":[]}"#].into()),
        ..ScriptedAi::default()
    });
    let sources = Arc::new(Sources::default());
    let (app, actors, conv) = fixture(&pool, ai.clone(), sources.clone()).await;
    let id = Uuid::parse_str(&conv).unwrap();
    let (activity, topic): (Uuid, Uuid) =
        sqlx::query_as("SELECT activity_id, topic_id FROM conversations WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let other_topic = Uuid::parse_str(&seed_topic("其他題目").await).unwrap();
    sqlx::query("INSERT INTO activity_topics (activity_id, topic_id, position) VALUES ($1, $2, 1)")
        .bind(activity)
        .bind(other_topic)
        .execute(&pool)
        .await
        .unwrap();
    let other = send(
        &app,
        json_req(
            "POST",
            "/api/conversations",
            Some(&actors.student),
            Some(json!({"activity_id": activity, "topic_id": other_topic})),
        ),
    )
    .await;
    assert_eq!(other.status(), StatusCode::CREATED);
    let other_body = json_body(other).await;
    let other_id = other_body["id"].as_str().unwrap();
    seed_student_message(&pool, other_id, "DISTINCT SECOND TOPIC DIALOGUE").await;
    let assigned = seed_book(&pool, None, "指定給完成題目的書", Some("zh")).await;
    put_on_activity_list(&pool, &conv, assigned.work_id).await;
    let only_default = seed_book(&pool, None, "僅在題目預設書單的書", Some("zh")).await;
    sqlx::query("INSERT INTO topic_books (topic_id, book_id) VALUES ($1, $2)")
        .bind(topic)
        .bind(only_default.work_id)
        .execute(&pool)
        .await
        .unwrap();
    let only_other = seed_book(&pool, None, "指定給另一題的書", Some("zh")).await;
    put_on_activity_list(&pool, other_id, only_other.work_id).await;

    finish_dialogue(&app, &actors, &conv).await;
    let result = settled(&app, &actors, &conv).await;
    assert_eq!(result["status"], "ready");
    assert_eq!(result["recommendations"].as_array().unwrap().len(), 1);
    assert_eq!(
        result["recommendations"][0]["editions"][0]["title"],
        "指定給完成題目的書"
    );
    assert_eq!(
        recommendation(&app, "GET", other_id, Some(&actors.student))
            .await
            .1["status"],
        "waiting_for_summary"
    );
    let reqs = ai.requests.lock().unwrap();
    assert_eq!(reqs.len(), 1);
    assert!(
        !reqs[0]
            .to_string()
            .contains("DISTINCT SECOND TOPIC DIALOGUE")
    );
    let candidates = reqs[0]["candidates"].as_array().unwrap();
    assert_eq!(
        candidates
            .iter()
            .filter(|book| book["teacher_recommended"] == true)
            .count(),
        1
    );
    assert_eq!(
        candidates.len(),
        3,
        "other lists do not prevent global catalog eligibility"
    );
    assert!(sources.calls.lock().unwrap().is_empty());
}

#[sqlx::test]
async fn removing_the_activities_enrollment_interrupts_an_eligible_students_job(pool: PgPool) {
    let gate = Arc::new(tokio::sync::Notify::new());
    let ai = Arc::new(ScriptedAi {
        choices: Mutex::new([COVERED].into()),
        wait_for: Some(gate.clone()),
        ..ScriptedAi::default()
    });
    let (app, actors, conv) = fixture(&pool, ai, Arc::default()).await;
    let elsewhere = create_classroom(&app, &actors.teacher, "另一個教室").await;
    let res = import_roster(
        &app,
        &actors.admin,
        &elsewhere,
        "學生,10000000A,student@example.com",
    )
    .await;
    assert!(res.status().is_success());
    let book = seed_book(&pool, None, "公平的原則", Some("zh")).await;
    put_on_activity_list(&pool, &conv, book.work_id).await;
    finish_dialogue(&app, &actors, &conv).await;

    sqlx::query("DELETE FROM enrollments WHERE classroom_id = $1")
        .bind(Uuid::parse_str(&actors.classroom).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let result = settled(&app, &actors, &conv).await;
    assert_eq!(result["status"], "interrupted");
    assert_eq!(result["can_retry"], false);
    assert_eq!(
        recommendation(&app, "POST", &conv, Some(&actors.student))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let res = import_roster(
        &app,
        &actors.admin,
        &actors.classroom,
        "學生,10000000A,student@example.com",
    )
    .await;
    assert!(res.status().is_success());
    gate.notify_one();
    let result = settled(&app, &actors, &conv).await;
    assert_eq!(result["status"], "interrupted");
    assert_eq!(result["can_retry"], true);
}

#[sqlx::test]
async fn removing_a_selected_edition_during_generation_discards_it(pool: PgPool) {
    let gate = Arc::new(tokio::sync::Notify::new());
    let ai = Arc::new(ScriptedAi {
        choices: Mutex::new([COVERED].into()),
        wait_for: Some(gate.clone()),
        ..ScriptedAi::default()
    });
    let (app, actors, conv) = fixture(&pool, ai.clone(), Arc::default()).await;
    let book = seed_book(&pool, None, "正義與選擇", Some("zh")).await;
    put_on_activity_list(&pool, &conv, book.work_id).await;
    finish_dialogue(&app, &actors, &conv).await;
    tokio::time::timeout(Duration::from_secs(1), async {
        while ai.requests.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    sqlx::query("DELETE FROM book_works WHERE id = $1")
        .bind(book.work_id)
        .execute(&pool)
        .await
        .unwrap();
    gate.notify_one();
    let result = settled(&app, &actors, &conv).await;
    assert_eq!(result["status"], "failed");
    assert_eq!(result["error_code"], "recommendation_invalid_output");
    assert_eq!(result["recommendations"], json!([]));
}

#[sqlx::test]
async fn recommendation_migration_can_run_again_after_fresh_install(pool: PgPool) {
    sqlx::migrate!().run(&pool).await.unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _sqlx_migrations WHERE version = 20261120000200 AND success",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test]
async fn empty_catalog_proposes_a_specific_book_then_verifies_it(pool: PgPool) {
    let sources = Arc::new(Sources {
        google: Some(vec![BookCandidate {
            authors: Some("合成作者".into()),
            ..external("Google Books", "正義與社會", "zh")
        }]),
        ..Sources::default()
    });
    let ai = Arc::new(ScriptedAi {
        choices: Mutex::new([r#"{"picks":[],"proposals":[{"title":"正義與社會","author":"合成作者"}]}"#, r#"{"picks":[{"id":"0","reason":"延伸公平的問題","question":"如何分配才公平？"}],"proposals":[]}"#].into()),
        ..ScriptedAi::default()
    });
    let (app, actors, conv) = fixture(&pool, ai.clone(), sources.clone()).await;
    finish_dialogue(&app, &actors, &conv).await;
    let result = settled(&app, &actors, &conv).await;
    assert_eq!(result["status"], "ready");
    assert_eq!(
        result["recommendations"][0]["editions"][0]["title"],
        "正義與社會"
    );
    assert_eq!(
        *sources.calls.lock().unwrap(),
        [("Google Books", "\"正義與社會\" \"合成作者\"".into())]
    );
    let requests = ai.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0]["candidates"], json!([]));
    assert_eq!(
        requests[1]["proposal"],
        json!({"title":"正義與社會","author":"合成作者"})
    );
    assert!(!sources.calls.lock().unwrap()[0].1.contains("synth-person"));
}

#[sqlx::test]
async fn unverified_title_author_or_derivative_is_never_recommended(pool: PgPool) {
    let books = [
        ("正義與社會", Some("同名書另一位作者")),
        ("正義與社會", None),
        ("正義與社會摘要導讀", Some("合成作者")),
    ]
    .into_iter()
    .map(|(title, author)| BookCandidate {
        authors: author.map(str::to_owned),
        ..external("Google Books", title, "zh")
    })
    .collect();
    let sources = Arc::new(Sources {
        google: Some(books),
        ol: Some(vec![]),
        ..Sources::default()
    });
    let ai = Arc::new(ScriptedAi {
        choices: Mutex::new([NEEDS_SOURCE].into()),
        ..ScriptedAi::default()
    });
    let (app, actors, conv) = fixture(&pool, ai.clone(), sources.clone()).await;
    finish_dialogue(&app, &actors, &conv).await;
    let result = settled(&app, &actors, &conv).await;
    assert_eq!(result["status"], "no_results");
    assert_eq!(result["recommendations"], json!([]));
    assert_eq!(sources.calls.lock().unwrap().len(), 2);
    assert_eq!(ai.requests.lock().unwrap().len(), 1);
}

#[sqlx::test]
async fn failed_verification_of_one_proposal_does_not_drop_another(pool: PgPool) {
    let sources = Arc::new(Sources {
        google: Some(vec![external("Google Books", "正義與社會", "zh")]),
        ol: Some(vec![]),
        ..Sources::default()
    });
    let ai = Arc::new(ScriptedAi {
        choices: Mutex::new([r#"{"picks":[],"proposals":[{"title":"查無此書","author":"合成作者"},{"title":"正義與社會","author":"合成作者"}]}"#, COVERED].into()),
        ..ScriptedAi::default()
    });
    let (app, actors, conv) = fixture(&pool, ai, sources.clone()).await;
    finish_dialogue(&app, &actors, &conv).await;
    let result = settled(&app, &actors, &conv).await;
    assert_eq!(result["status"], "ready");
    assert_eq!(result["recommendations"].as_array().unwrap().len(), 1);
    assert_eq!(
        result["recommendations"][0]["editions"][0]["title"],
        "正義與社會"
    );
    assert_eq!(
        *sources.calls.lock().unwrap(),
        [
            ("Google Books", "\"查無此書\" \"合成作者\"".into()),
            ("Open Library", "\"查無此書\" \"合成作者\"".into()),
            ("Google Books", "\"正義與社會\" \"合成作者\"".into()),
        ]
    );
}

#[sqlx::test]
async fn no_reliable_model_proposals_is_saved_without_external_queries(pool: PgPool) {
    let sources = Arc::new(Sources::default());
    let ai = Arc::new(ScriptedAi {
        choices: Mutex::new([REJECT_ALL].into()),
        ..ScriptedAi::default()
    });
    let (app, actors, conv) = fixture(&pool, ai, sources.clone()).await;
    finish_dialogue(&app, &actors, &conv).await;
    let result = settled(&app, &actors, &conv).await;
    assert_eq!(result["status"], "no_results");
    assert!(sources.calls.lock().unwrap().is_empty());
}

#[sqlx::test]
async fn identifiable_personal_information_in_proposals_never_leaves_for_book_sources(
    pool: PgPool,
) {
    let sources = Arc::new(Sources::default());
    let ai = Arc::new(ScriptedAi {
        choices: Mutex::new([r#"{"picks":[],"proposals":[{"title":"synth-person@example.test","author":"合成作者"}]}"#].into()),
        ..ScriptedAi::default()
    });
    let (app, actors, conv) = fixture(&pool, ai, sources.clone()).await;
    finish_dialogue(&app, &actors, &conv).await;
    let result = settled(&app, &actors, &conv).await;
    assert_eq!(result["status"], "failed");
    assert_eq!(result["error_code"], "recommendation_invalid_output");
    assert!(sources.calls.lock().unwrap().is_empty());
}

#[sqlx::test]
async fn multiple_verified_proposals_preserve_model_order(pool: PgPool) {
    let sources = Arc::new(Sources {
        google: Some(
            ["先選的書", "後選的書"]
                .into_iter()
                .map(|title| BookCandidate {
                    source_id: title.into(),
                    ..external("Google Books", title, "zh")
                })
                .collect(),
        ),
        ..Sources::default()
    });
    let ai = Arc::new(ScriptedAi {
        choices: Mutex::new([r#"{"picks":[],"proposals":[{"title":"先選的書","author":"合成作者"},{"title":"後選的書","author":"合成作者"}]}"#, COVERED, COVERED].into()),
        ..ScriptedAi::default()
    });
    let (app, actors, conv) = fixture(&pool, ai, sources).await;
    finish_dialogue(&app, &actors, &conv).await;
    let result = settled(&app, &actors, &conv).await;
    assert_eq!(result["status"], "ready");
    assert_eq!(
        result["recommendations"][0]["editions"][0]["title"],
        "先選的書"
    );
    assert_eq!(
        result["recommendations"][1]["editions"][0]["title"],
        "後選的書"
    );
}
