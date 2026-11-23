use async_trait::async_trait;
use socrates_chat_backend::{
    book_import::import_csv,
    book_sources::{BookCandidate, BookSource, BookSourceError},
};
use sqlx::PgPool;
struct NoResults;
#[sqlx::test]
async fn import_preserves_external_metadata_provenance(pool: PgPool) {
    import_csv(
        &pool,
        "title\nSynthetic provenance\n",
        &Candidates(vec![candidate("Synthetic provenance")]),
    )
    .await
    .unwrap();
    let (sources,): (serde_json::Value,) = sqlx::query_as("SELECT external_sources FROM books")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        sources,
        serde_json::json!([{"source":"Google Books","source_id":"synthetic-source-id"}])
    );
}
#[async_trait]
impl BookSource for NoResults {
    async fn search(&self, _: &str) -> Result<Vec<BookCandidate>, BookSourceError> {
        Ok(vec![])
    }
}
#[sqlx::test]
async fn import_title_only_and_duplicate_rows_without_associations(pool: PgPool) {
    let report = import_csv(
        &pool,
        "title,authors\n  Synthetic   Ethics ,\nsynthetic ethics,\nAnother book,Synthetic Writer\n",
        &NoResults,
    )
    .await
    .unwrap();
    assert_eq!((report.added, report.skipped, report.failed), (2, 1, 0));
    let (books,): (i64,) = sqlx::query_as("SELECT count(*) FROM books WHERE created_by IS NULL")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(books, 2);
    let (works, linked): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM book_works),
         (SELECT count(*) FROM books b JOIN book_works w ON b.work_id=w.id)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((works, linked), (2, 2));
    let again = import_csv(
        &pool,
        "title,authors\nSynthetic Ethics,\nAnother book,Synthetic Writer\n",
        &NoResults,
    )
    .await
    .unwrap();
    assert_eq!((again.added, again.skipped, again.failed), (0, 2, 0));
    let (after,): (i64,) = sqlx::query_as("SELECT count(*) FROM book_works")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(after, 2, "duplicate imports must not leave orphan works");

    let (links,): (i64,) = sqlx::query_as("SELECT count(*) FROM topic_books")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(links, 0);
}

use socrates_chat_backend::{book_import::ImportError, book_store};

struct Candidates(Vec<BookCandidate>);
#[async_trait]
impl BookSource for Candidates {
    async fn search(&self, _: &str) -> Result<Vec<BookCandidate>, BookSourceError> {
        Ok(self.0.clone())
    }
}
struct SourceFailure;
#[async_trait]
impl BookSource for SourceFailure {
    async fn search(&self, _: &str) -> Result<Vec<BookCandidate>, BookSourceError> {
        Err(BookSourceError)
    }
}
fn candidate(title: &str) -> BookCandidate {
    BookCandidate {
        source: "Google Books".into(),
        source_id: "synthetic-source-id".into(),
        title: title.into(),
        authors: Some("External Writer".into()),
        language: Some("en".into()),
        publication_year: Some("2025".into()),
        isbn: None,
        source_url: Some("https://example.com/synthetic-book".into()),
        description: None,
    }
}

#[sqlx::test]
async fn import_isbn_and_author_normalization_deduplicates_without_overwriting(pool: PgPool) {
    let report = import_csv(
        &pool,
        concat!(
            "title,authors,isbn\n",
            "Twin,Ada; Bo,0306406152\n",
            "Different supplied title,Other Writer,978-0-306-40615-7\n",
            "Twin,Bo; Ada,9780000000019\n",
            "  twin  , bo ; ADA ,\n",
            "Twin,Someone Else,\n",
            "Twin,,\n",
        ),
        &NoResults,
    )
    .await
    .unwrap();
    assert_eq!((report.added, report.skipped, report.failed), (3, 3, 0));
    let books = book_store::list(&pool, None, None, 100).await.unwrap();
    assert_eq!(books.len(), 3);
    let original = books
        .iter()
        .find(|b| b.isbn.as_deref() == Some("9780306406157"))
        .unwrap();
    assert_eq!(original.title, "Twin");
    assert_eq!(original.authors.as_deref(), Some("Ada; Bo"));
    assert!(
        books
            .iter()
            .any(|b| b.isbn.as_deref() == Some("9780000000019"))
    );
    assert!(
        books
            .iter()
            .any(|b| b.authors.as_deref() == Some("Someone Else"))
    );
}

#[sqlx::test]
async fn import_invalid_original_fields_reports_lines_and_continues(pool: PgPool) {
    let csv = format!(
        "title,isbn,source_url\nValid,,https://example.com/book\n,123,\nBad ISBN,not-an-isbn,\nBad URL,,javascript:alert(1)\n{},,\n",
        "書".repeat(256)
    );
    let report = import_csv(&pool, &csv, &NoResults).await.unwrap();
    assert_eq!(report.added, 1);
    assert_eq!(report.skipped + report.failed, 4);
    for line in 3..=6 {
        let row = report.rows.iter().find(|r| r.line == line).unwrap();
        assert!(!row.status.starts_with("added"));
        assert!(row.reason.is_some());
    }
    assert_eq!(
        book_store::list(&pool, None, None, 100)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[sqlx::test]
async fn ambiguous_and_failed_sources_keep_valid_original_rows(pool: PgPool) {
    let ambiguous = Candidates(vec![candidate("Ambiguous"), candidate("Ambiguous")]);
    let report = import_csv(
        &pool,
        "title,authors\nAmbiguous,Original Writer\n",
        &ambiguous,
    )
    .await
    .unwrap();
    assert_eq!((report.added, report.skipped, report.failed), (1, 0, 0));
    assert_eq!(report.rows[0].status, "added_needs_metadata");
    assert!(report.rows[0].reason.is_some());
    let report = import_csv(
        &pool,
        "title,authors\nService failure,Original Writer\n",
        &SourceFailure,
    )
    .await
    .unwrap();
    assert_eq!((report.added, report.skipped, report.failed), (1, 0, 0));
    assert_eq!(report.rows[0].status, "added_needs_metadata");
    assert!(report.rows[0].reason.is_some());
    for book in book_store::list(&pool, None, None, 100).await.unwrap() {
        assert_eq!(book.authors.as_deref(), Some("Original Writer"));
        assert!(book.language.is_none());
        assert!(book.source_url.is_none());
    }
}

#[sqlx::test]
async fn supplements_fill_only_missing_fields_and_ignore_oversized_metadata(pool: PgPool) {
    let mut result = candidate("Original title");
    result.authors = Some("Original Writer".into());
    result.description = Some("字".repeat(5001));
    let report = import_csv(
        &pool,
        "title,authors\nOriginal title,Original Writer\n",
        &Candidates(vec![result]),
    )
    .await
    .unwrap();
    assert_eq!((report.added, report.skipped, report.failed), (1, 0, 0));
    assert_eq!(report.rows[0].status, "added_needs_metadata");
    assert!(report.rows[0].reason.is_some());
    let books = book_store::list(&pool, None, None, 100).await.unwrap();
    assert_eq!(books[0].title, "Original title");
    assert_eq!(books[0].authors.as_deref(), Some("Original Writer"));
    assert!(books[0].description.is_none());
    assert_eq!(books[0].language.as_deref(), Some("en"));
    assert_eq!(books[0].publication_year.as_deref(), Some("2025"));
}

#[sqlx::test]
async fn a_second_import_is_rejected_while_first_holds_the_job_lock(pool: PgPool) {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(socrates_chat_backend::book_import::IMPORT_LOCK)
        .execute(&mut *tx)
        .await
        .unwrap();
    let result = import_csv(&pool, "title\nConcurrent\n", &NoResults).await;
    assert!(matches!(result, Err(ImportError::Busy)));
    tx.rollback().await.unwrap();
    assert_eq!(
        import_csv(&pool, "title\nAfter lock release\n", &NoResults)
            .await
            .unwrap()
            .added,
        1
    );
}

#[sqlx::test]
async fn a_conflicting_candidate_does_not_mix_another_authors_edition_into_the_original(
    pool: PgPool,
) {
    let mut result = candidate("Same title");
    result.authors = Some("Different Author".into());
    result.isbn = Some("9780306406157".into());
    result.description = Some("A different author's book.".into());
    let report = import_csv(
        &pool,
        "title,authors\nSame title,Original Author\n",
        &Candidates(vec![result]),
    )
    .await
    .unwrap();
    assert_eq!((report.added, report.skipped, report.failed), (1, 0, 0));
    assert_eq!(report.rows[0].status, "added_needs_metadata");
    let books = book_store::list(&pool, None, None, 100).await.unwrap();
    assert_eq!(books[0].authors.as_deref(), Some("Original Author"));
    assert!(books[0].isbn.is_none());
    assert!(books[0].description.is_none());
    assert!(books[0].language.is_none());
}

#[sqlx::test]
async fn a_conflicting_language_does_not_supplement_a_different_translation(pool: PgPool) {
    let result = candidate("Same title"); // The external edition is English.
    let report = import_csv(
        &pool,
        "title,language\nSame title,es\n",
        &Candidates(vec![result]),
    )
    .await
    .unwrap();
    assert_eq!(report.added, 1);
    assert_eq!(report.rows[0].status, "added_needs_metadata");
    assert_eq!(report.rows[0].reason.as_deref(), Some("version_conflict"));
    let books = book_store::list(&pool, None, None, 100).await.unwrap();
    assert_eq!(books[0].language.as_deref(), Some("es"));
    assert!(books[0].authors.is_none());
    assert!(books[0].publication_year.is_none());
}

#[sqlx::test]
async fn an_unconfirmed_candidate_does_not_supply_metadata_for_a_specific_isbn(pool: PgPool) {
    let result = candidate("Specific edition"); // Missing ISBN cannot corroborate this edition.
    let report = import_csv(
        &pool,
        "title,isbn\nSpecific edition,9780306406157\n",
        &Candidates(vec![result]),
    )
    .await
    .unwrap();
    assert_eq!(report.added, 1);
    assert_eq!(report.rows[0].status, "added_needs_metadata");
    assert_eq!(
        report.rows[0].reason.as_deref(),
        Some("version_unconfirmed")
    );
    let books = book_store::list(&pool, None, None, 100).await.unwrap();
    assert_eq!(books[0].isbn.as_deref(), Some("9780306406157"));
    assert!(books[0].authors.is_none());
    assert!(books[0].publication_year.is_none());
}

#[sqlx::test]
async fn equivalent_language_codes_do_not_block_csv_supplementation(pool: PgPool) {
    let mut external = candidate("Synthetic aliases");
    external.language = Some("eng".into());
    let report = import_csv(
        &pool,
        "title,language\nSynthetic aliases,英文\n",
        &Candidates(vec![external]),
    )
    .await
    .unwrap();
    assert_eq!(report.added, 1);
    assert_eq!(report.rows[0].reason, None);
    let books = book_store::list(&pool, None, None, 10).await.unwrap();
    assert_eq!(books[0].language.as_deref(), Some("en"));
    assert!(books[0].authors.is_some());
}
