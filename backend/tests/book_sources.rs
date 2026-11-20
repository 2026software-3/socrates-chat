use axum::{
    Json, Router,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde_json::{Value, json};
use socrates_chat_backend::book_sources::{BookSource, HttpBookSource};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

async fn server(google_status: StatusCode, google: Value, ol: Value) -> (String, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let app = Router::new()
        .route(
            "/volumes",
            get(move |headers: HeaderMap| {
                let body = google.clone();
                async move {
                    assert_eq!(headers.get("x-goog-api-key").unwrap(), "synthetic-test-key");
                    (google_status, Json(body))
                }
            }),
        )
        .route(
            "/search.json",
            get(move || {
                let body = ol.clone();
                let calls = counted.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Json(body)
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, calls)
}

#[tokio::test]
async fn usable_google_book_does_not_call_fallback() {
    let (url,calls)=server(StatusCode::OK,json!({"items":[{"id":"synthetic-volume","volumeInfo":{"title":"Synthetic ethics","authors":["Synthetic Author"],"publishedDate":"2025-02-01","language":"en","industryIdentifiers":[{"type":"ISBN_13","identifier":"9780140449334"}]}}]}),json!({"docs":[]})).await;
    let provider = HttpBookSource::with_urls(
        Some("synthetic-test-key".into()),
        format!("{url}/volumes"),
        format!("{url}/search.json"),
    );
    let books = provider.search("Synthetic ethics").await.unwrap();
    assert_eq!(books.len(), 1);
    assert_eq!(books[0].source, "Google Books");
    assert_eq!(books[0].publication_year.as_deref(), Some("2025"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

/// Fake source endpoints accept no real credentials and expose independent request counters.
async fn detailed_server(
    google_status: StatusCode,
    google_body: String,
    ol_status: StatusCode,
    ol_body: Value,
    edition: Value,
) -> (String, Arc<AtomicUsize>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let google_calls = Arc::new(AtomicUsize::new(0));
    let ol_calls = Arc::new(AtomicUsize::new(0));
    let edition_calls = Arc::new(AtomicUsize::new(0));
    let g = google_calls.clone();
    let o = ol_calls.clone();
    let e = edition_calls.clone();
    let app = Router::new()
        .route(
            "/volumes",
            get(move |headers: HeaderMap| {
                let body = google_body.clone();
                let calls = g.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(headers.get("x-goog-api-key").unwrap(), "synthetic-test-key");
                    (google_status, body)
                }
            }),
        )
        .route(
            "/search.json",
            get(move |headers: HeaderMap| {
                let body = ol_body.clone();
                let calls = o.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    assert!(headers.get("x-goog-api-key").is_none());
                    (ol_status, Json(body))
                }
            }),
        )
        .route(
            "/books/{edition}",
            get(move |headers: HeaderMap| {
                let body = edition.clone();
                let calls = e.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    assert!(headers.get("x-goog-api-key").is_none());
                    Json(body)
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, google_calls, ol_calls, edition_calls)
}
fn provider(url: &str, key: bool) -> HttpBookSource {
    HttpBookSource::with_urls(
        key.then(|| "synthetic-test-key".into()),
        format!("{url}/volumes"),
        format!("{url}/search.json"),
    )
}
fn ol_result() -> Value {
    json!({"docs":[{"key":"/works/OL42W","title":"Synthetic ethics","author_name":["WORK author must not become edition author"],"isbn":["9780140449334"],"language":["spa"],"first_publish_year":1900,"editions":{"docs":[{"key":"/books/OL42M"}]}}]})
}
fn edition_result() -> Value {
    json!({"key":"/books/OL42M","title":"Synthetic ethics","publish_date":"2024","isbn_13":["9780306406157"],"languages":[{"key":"/languages/eng"}],"description":{"value":"Synthetic edition description"}})
}

#[tokio::test]
async fn zero_unrelated_or_unidentifiable_google_candidates_use_verified_ol_edition() {
    for google in [
        json!({"totalItems":0}),
        json!({"items":[{"id":"other-volume","volumeInfo":{"title":"Synthetic gardening"}}]}),
        json!({"items":[{"volumeInfo":{"title":"Synthetic ethics"}}]}),
    ] {
        let (url, g, o, e) = detailed_server(
            StatusCode::OK,
            google.to_string(),
            StatusCode::OK,
            ol_result(),
            edition_result(),
        )
        .await;
        let rows = provider(&url, true)
            .search("Synthetic ethics")
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.source, "Open Library");
        assert_eq!(row.source_id, "/books/OL42M");
        assert_eq!(row.language.as_deref(), Some("eng"));
        assert_eq!(row.publication_year.as_deref(), Some("2024"));
        assert_eq!(row.isbn.as_deref(), Some("9780306406157"));
        assert!(row.authors.is_none());
        assert_eq!(g.load(Ordering::SeqCst), 1);
        assert_eq!(o.load(Ordering::SeqCst), 1);
        assert_eq!(e.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn google_errors_and_malformed_payloads_do_not_trigger_fallback() {
    for (status, body) in [
        (
            StatusCode::TOO_MANY_REQUESTS,
            json!({"error":"synthetic quota failure"}).to_string(),
        ),
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "synthetic internal failure".into(),
        ),
        (StatusCode::OK, "not-json".into()),
        (StatusCode::OK, json!({"items":{}}).to_string()),
        (StatusCode::OK, json!({}).to_string()),
        (StatusCode::OK, json!({"error":{"code":403}}).to_string()),
    ] {
        let (url, g, o, e) =
            detailed_server(status, body, StatusCode::OK, ol_result(), edition_result()).await;
        assert!(
            provider(&url, true)
                .search("Synthetic ethics")
                .await
                .is_err()
        );
        assert_eq!(g.load(Ordering::SeqCst), 1);
        assert_eq!(o.load(Ordering::SeqCst), 0);
        assert_eq!(e.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn missing_key_makes_no_external_request_and_fallback_failure_is_not_empty_success() {
    let (url, g, o, _) = detailed_server(
        StatusCode::OK,
        json!({"totalItems":0}).to_string(),
        StatusCode::SERVICE_UNAVAILABLE,
        json!({}),
        json!({}),
    )
    .await;
    assert!(
        provider(&url, false)
            .search("Synthetic ethics")
            .await
            .is_err()
    );
    assert_eq!(g.load(Ordering::SeqCst), 0);
    assert_eq!(o.load(Ordering::SeqCst), 0);
    assert!(
        provider(&url, true)
            .search("Synthetic ethics")
            .await
            .is_err()
    );
    assert_eq!(g.load(Ordering::SeqCst), 1);
    assert_eq!(o.load(Ordering::SeqCst), 1);
    let (url, _, o, _) = detailed_server(
        StatusCode::OK,
        json!({"totalItems":0}).to_string(),
        StatusCode::OK,
        json!({"docs":{}}),
        json!({}),
    )
    .await;
    assert!(
        provider(&url, true)
            .search("Synthetic ethics")
            .await
            .is_err()
    );
    assert_eq!(o.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn both_sources_empty_is_normal_and_isbn_queries_verify_requested_version() {
    let (url, _, o, _) = detailed_server(
        StatusCode::OK,
        json!({"totalItems":0}).to_string(),
        StatusCode::OK,
        json!({"docs":[]}),
        json!({}),
    )
    .await;
    assert!(
        provider(&url, true)
            .search("Synthetic ethics")
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(o.load(Ordering::SeqCst), 1);
    let google = json!({"items":[{"id":"synthetic-isbn","volumeInfo":{"title":"Synthetic ethics","industryIdentifiers":[{"type":"ISBN_10","identifier":"0306406152"}]}}]});
    let (url, _, o, _) = detailed_server(
        StatusCode::OK,
        google.to_string(),
        StatusCode::OK,
        json!({"docs":[]}),
        json!({}),
    )
    .await;
    assert_eq!(
        provider(&url, true)
            .search("978-0-306-40615-7")
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(o.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn malformed_or_mismatched_ol_edition_ids_are_not_verified_candidates() {
    let mut result = ol_result();
    result["docs"][0]["editions"]["docs"][0]["key"] = json!("/books/OLM");
    let (url, _, _, e) = detailed_server(
        StatusCode::OK,
        json!({"totalItems":0}).to_string(),
        StatusCode::OK,
        result,
        edition_result(),
    )
    .await;
    assert!(
        provider(&url, true)
            .search("Synthetic ethics")
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        e.load(Ordering::SeqCst),
        0,
        "a malformed edition ID must not be fetched"
    );
    let mut wrong = edition_result();
    wrong["key"] = json!("/books/OL99M");
    let (url, _, _, _) = detailed_server(
        StatusCode::OK,
        json!({"totalItems":0}).to_string(),
        StatusCode::OK,
        ol_result(),
        wrong,
    )
    .await;
    assert!(
        provider(&url, true)
            .search("Synthetic ethics")
            .await
            .unwrap()
            .is_empty(),
        "a different returned edition must not be relabelled as the requested one"
    );
}

async fn delayed_server(
    google_delay: std::time::Duration,
    ol_delay: std::time::Duration,
) -> (String, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let app = Router::new()
        .route(
            "/volumes",
            get(move || async move {
                tokio::time::sleep(google_delay).await;
                Json(json!({"totalItems":0}))
            }),
        )
        .route(
            "/search.json",
            get(move || {
                let calls = counted.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(ol_delay).await;
                    Json(json!({"docs":[]}))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, calls)
}

#[tokio::test]
async fn google_timeout_is_failure_and_does_not_start_openlibrary() {
    use std::time::Duration;
    let (url, calls) = delayed_server(Duration::from_millis(200), Duration::ZERO).await;
    let result = provider(&url, true)
        .with_provider_timeout(Duration::from_millis(40))
        .search("Synthetic ethics")
        .await;
    assert!(result.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn openlibrary_budget_includes_search_edition_verification_and_courtesy_wait() {
    use std::time::Duration;
    let (url, _, calls, editions) = detailed_server(
        StatusCode::OK,
        json!({"totalItems":0}).to_string(),
        StatusCode::OK,
        ol_result(),
        edition_result(),
    )
    .await;
    let source = provider(&url, true).with_provider_timeout(Duration::from_millis(40));
    let result = tokio::time::timeout(
        Duration::from_millis(500),
        source.search("Synthetic ethics"),
    )
    .await;
    assert!(
        result.is_ok(),
        "the provider budget must include the courtesy queue, not restart for each HTTP request"
    );
    assert!(result.unwrap().is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        editions.load(Ordering::SeqCst),
        0,
        "cancel before sending an edition request after its budget expired"
    );
}

#[tokio::test]
async fn google_and_openlibrary_each_receive_their_own_timeout_budget() {
    use std::time::Duration;
    // Two 200 ms requests exceed one 350 ms budget, but each source finishes within its own.
    let (url, calls) = delayed_server(Duration::from_millis(200), Duration::from_millis(200)).await;
    let result = provider(&url, true)
        .with_provider_timeout(Duration::from_millis(350))
        .search("Synthetic ethics")
        .await;
    assert!(result.unwrap().is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
