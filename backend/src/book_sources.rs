//! Teacher book lookup. Credentials stay server-side; provider failures never trigger fallback.
use crate::diagnostics::Failure;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct BookCandidate {
    pub source: String,
    pub source_id: String,
    pub title: String,
    pub authors: Option<String>,
    pub language: Option<String>,
    pub publication_year: Option<String>,
    pub isbn: Option<String>,
    pub source_url: Option<String>,
    pub description: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookProvider {
    GoogleBooks,
    OpenLibrary,
}
impl BookProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GoogleBooks => "google_books",
            Self::OpenLibrary => "open_library",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BookSourceError {
    pub provider: BookProvider,
    pub class: Failure,
    pub http_status: Option<u16>,
}
impl BookSourceError {
    pub fn new(provider: BookProvider, class: Failure) -> Self {
        Self {
            provider,
            class,
            http_status: None,
        }
    }
    pub fn http(provider: BookProvider, http_status: u16, class: Failure) -> Self {
        Self {
            provider,
            class,
            http_status: Some(http_status),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RelatedEdition {
    pub work_id: String,
    pub work_title: String,
    pub book: BookCandidate,
}
#[async_trait]
pub trait BookSource: Send + Sync {
    async fn search(&self, query: &str) -> Result<Vec<BookCandidate>, BookSourceError>;
    async fn related(
        &self,
        _book: &crate::book_store::Book,
    ) -> Result<Vec<RelatedEdition>, BookSourceError> {
        Err(BookSourceError::new(
            BookProvider::OpenLibrary,
            Failure::ServiceError,
        ))
    }
}
pub struct HttpBookSource {
    client: reqwest::Client,
    key: Option<String>,
    provider_timeout: Duration,
    google: String,
    openlibrary: String,
    ol_last_request: Arc<Mutex<Option<tokio::time::Instant>>>,
}
impl HttpBookSource {
    pub fn new(key: Option<String>) -> Self {
        Self::with_urls(
            key,
            "https://www.googleapis.com/books/v1/volumes".into(),
            "https://openlibrary.org/search.json".into(),
        )
    }
    /// Endpoint injection for local fake-server tests; never accepted from API requests.
    pub fn with_urls(key: Option<String>, google: String, openlibrary: String) -> Self {
        Self {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(10))
                .user_agent(
                    "socrates-chat-book-lookup (https://github.com/2026software-3/socrates-chat)",
                )
                .build()
                .expect("static HTTP client configuration"),
            key,
            provider_timeout: Duration::from_secs(10),
            google,
            openlibrary,
            ol_last_request: Arc::default(),
        }
    }
    /// Shorter budgets for deterministic fake-server tests; production defaults to ten seconds.
    pub fn with_provider_timeout(mut self, timeout: Duration) -> Self {
        self.provider_timeout = timeout;
        self
    }

    async fn json(
        &self,
        request: reqwest::RequestBuilder,
        provider: BookProvider,
    ) -> Result<Value, BookSourceError> {
        if provider == BookProvider::OpenLibrary {
            let mut last = self.ol_last_request.lock().await;
            if let Some(at) = *last {
                tokio::time::sleep_until(at + Duration::from_millis(1100)).await;
            }
            *last = Some(tokio::time::Instant::now());
        }
        let mut response = request
            .timeout(self.provider_timeout)
            .send()
            .await
            .map_err(|err| {
                BookSourceError::new(
                    provider,
                    if err.is_timeout() {
                        Failure::Timeout
                    } else {
                        Failure::ConnectionFailed
                    },
                )
            })?;
        let status = response.status();
        if !status.is_success() {
            let code = status.as_u16();
            let class = if provider == BookProvider::GoogleBooks && matches!(code, 400 | 401 | 403)
            {
                let mut bytes = Vec::new();
                while let Ok(Some(chunk)) = response.chunk().await {
                    if bytes.len() + chunk.len() > 64 * 1024 {
                        break;
                    }
                    bytes.extend_from_slice(&chunk);
                }
                classify_google_error(code, &bytes)
            } else if code == 429 {
                Failure::RateLimited
            } else {
                Failure::ServiceError
            };
            return Err(BookSourceError::http(provider, code, class));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|err| {
            BookSourceError::new(
                provider,
                if err.is_timeout() {
                    Failure::Timeout
                } else {
                    Failure::ConnectionFailed
                },
            )
        })? {
            if bytes.len() + chunk.len() > 1_000_000 {
                return Err(BookSourceError::new(provider, Failure::InvalidOutput));
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes)
            .map_err(|_| BookSourceError::new(provider, Failure::InvalidOutput))
    }
    async fn lookup(&self, query: &str) -> Result<Vec<BookCandidate>, BookSourceError> {
        let Some(key) = self.key.as_deref().filter(|s| !s.trim().is_empty()) else {
            return tokio::time::timeout(self.provider_timeout, self.lookup_openlibrary(query))
                .await
                .map_err(|_| BookSourceError::new(BookProvider::OpenLibrary, Failure::Timeout))?;
        };
        let payload = self
            .json(
                self.client
                    .get(&self.google)
                    .header("X-Goog-Api-Key", key)
                    .query(&[("q", query), ("maxResults", "5"), ("printType", "books")]),
                BookProvider::GoogleBooks,
            )
            .await?;
        // A malformed response is a failure, not a reason to switch providers.
        if !payload.is_object() || payload.get("error").is_some() {
            return Err(BookSourceError::new(
                BookProvider::GoogleBooks,
                Failure::InvalidOutput,
            ));
        }
        let empty = Vec::new();
        let items = match payload.get("items") {
            Some(v) => v.as_array().ok_or_else(|| {
                BookSourceError::new(BookProvider::GoogleBooks, Failure::InvalidOutput)
            })?,
            None if payload.get("totalItems").and_then(Value::as_u64) == Some(0) => &empty,
            None => {
                return Err(BookSourceError::new(
                    BookProvider::GoogleBooks,
                    Failure::InvalidOutput,
                ));
            }
        };
        let found: Vec<_> = items
            .iter()
            .filter_map(google_candidate)
            .filter(|b| matches_query(b, query))
            .collect();
        if !found.is_empty() {
            return Ok(found);
        }
        // The fallback is one provider operation: its courtesy queue, search and edition
        // verification share a budget. Google consumed a separate budget before this point.
        tokio::time::timeout(self.provider_timeout, self.lookup_openlibrary(query))
            .await
            .map_err(|_| BookSourceError::new(BookProvider::OpenLibrary, Failure::Timeout))?
    }

    async fn lookup_openlibrary(&self, query: &str) -> Result<Vec<BookCandidate>, BookSourceError> {
        let payload = self
            .json(
                self.client.get(&self.openlibrary).query(&[
                    ("q", query),
                    ("limit", "5"),
                    (
                        "fields",
                        "key,title,author_name,editions,editions.key,editions.title,editions.language",
                    ),
                ]),
                BookProvider::OpenLibrary,
            )
            .await?;
        let docs = payload
            .get("docs")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                BookSourceError::new(BookProvider::OpenLibrary, Failure::InvalidOutput)
            })?;
        let mut results = Vec::new();
        for work in docs.iter().take(5) {
            let Some(edition) = work.pointer("/editions/docs/0/key").and_then(Value::as_str) else {
                continue;
            };
            let code = edition.strip_prefix("/books/").unwrap_or(edition);
            if code.len() < 4
                || !code.starts_with("OL")
                || !code.ends_with('M')
                || !code[2..code.len() - 1].chars().all(|c| c.is_ascii_digit())
            {
                continue;
            }
            let mut url = reqwest::Url::parse(&self.openlibrary).map_err(|_| {
                BookSourceError::new(BookProvider::OpenLibrary, Failure::ServiceError)
            })?;
            url.set_path(&format!("/books/{code}.json"));
            url.set_query(None);
            let data = self
                .json(self.client.get(url), BookProvider::OpenLibrary)
                .await?;
            // A returned edition must identify the requested edition, not a different record.
            let expected_key = format!("/books/{code}");
            if data.get("key").and_then(Value::as_str) != Some(expected_key.as_str()) {
                continue;
            }
            let Some(title) = text(&data, "title") else {
                continue;
            };
            // Do not assign work-level authors/languages/ISBNs to a specific edition.
            let authors = None;
            let candidate = BookCandidate {
                source: "Open Library".into(),
                source_id: format!("/books/{code}"),
                title,
                authors,
                language: data
                    .pointer("/languages/0/key")
                    .and_then(Value::as_str)
                    .and_then(|v| v.strip_prefix("/languages/"))
                    .map(str::to_owned),
                publication_year: text(&data, "publish_date").and_then(|s| year(&s)),
                isbn: data
                    .get("isbn_13")
                    .or_else(|| data.get("isbn_10"))
                    .and_then(Value::as_array)
                    .and_then(|a| a.iter().find_map(|s| s.as_str().and_then(isbn13))),
                source_url: Some(format!("https://openlibrary.org/books/{code}")),
                description: description(&data),
            };
            if matches_query(&candidate, query)
                || work
                    .get("author_name")
                    .and_then(Value::as_array)
                    .is_some_and(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .any(|name| name.to_lowercase().contains(&query.to_lowercase()))
                    })
            {
                results.push(candidate)
            }
        }
        Ok(results)
    }
}
fn log_lookup_failure(err: BookSourceError) {
    tracing::warn!(
        provider = err.provider.as_str(),
        class = err.class.code(),
        http_status = err.http_status,
        "external book source lookup failed"
    );
}
#[derive(Deserialize)]
struct GoogleErrorResponse {
    #[serde(default)]
    error: GoogleErrorBody,
}
#[derive(Default, Deserialize)]
struct GoogleErrorBody {
    #[serde(default)]
    message: String,
    #[serde(default)]
    errors: Vec<GoogleErrorReason>,
    #[serde(default)]
    details: Vec<GoogleErrorDetail>,
}
#[derive(Deserialize)]
struct GoogleErrorReason {
    #[serde(default)]
    reason: String,
}
#[derive(Deserialize)]
struct GoogleErrorDetail {
    #[serde(default)]
    reason: String,
}
fn classify_google_error(status: u16, body: &[u8]) -> Failure {
    const QUOTA_REASONS: [&str; 3] = [
        "dailyLimitExceeded",
        "userRateLimitExceeded",
        "RATE_LIMIT_EXCEEDED",
    ];
    const QUOTA_MESSAGES: [&str; 2] = ["Daily Limit Exceeded", "User Rate Limit Exceeded"];
    const KEY_REASONS: [&str; 2] = ["API_KEY_INVALID", "keyInvalid"];

    let parsed = serde_json::from_slice::<GoogleErrorResponse>(body).ok();
    if let Some(parsed) = parsed {
        let reasons = parsed
            .error
            .errors
            .iter()
            .map(|item| item.reason.as_str())
            .chain(parsed.error.details.iter().map(|d| d.reason.as_str()))
            .collect::<Vec<_>>();
        let is_quota = QUOTA_MESSAGES
            .iter()
            .any(|msg| parsed.error.message.eq_ignore_ascii_case(msg))
            || reasons.iter().any(|r| QUOTA_REASONS.contains(r));
        if is_quota {
            return Failure::QuotaExceeded;
        }
        if status == 400
            && (reasons.iter().any(|r| KEY_REASONS.contains(r))
                || parsed.error.message.contains("API key not valid"))
        {
            return Failure::InvalidKey;
        }
    }
    if matches!(status, 401 | 403) {
        Failure::InvalidKey
    } else {
        Failure::ServiceError
    }
}
#[async_trait]
impl BookSource for HttpBookSource {
    async fn related(
        &self,
        book: &crate::book_store::Book,
    ) -> Result<Vec<RelatedEdition>, BookSourceError> {
        let result = tokio::time::timeout(self.provider_timeout, self.lookup_related(book))
            .await
            .map_err(|_| BookSourceError::new(BookProvider::OpenLibrary, Failure::Timeout))
            .and_then(|res| res);
        if let Err(err) = &result {
            log_lookup_failure(*err);
        }
        result
    }
    async fn search(&self, query: &str) -> Result<Vec<BookCandidate>, BookSourceError> {
        let result = self.lookup(query.trim()).await;
        if let Err(err) = &result {
            log_lookup_failure(*err);
        }
        result
    }
}
fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}
fn year(s: &str) -> Option<String> {
    s.split(|c: char| !c.is_ascii_digit())
        .find(|s| s.len() == 4)
        .map(str::to_owned)
}
fn description(v: &Value) -> Option<String> {
    text(v, "description").or_else(|| {
        v.pointer("/description/value")
            .and_then(Value::as_str)
            .map(str::to_owned)
    })
}
fn google_candidate(item: &Value) -> Option<BookCandidate> {
    let id = text(item, "id")?;
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    let v = item.get("volumeInfo")?;
    let title = text(v, "title")?;
    Some(BookCandidate {
        source: "Google Books".into(),
        source_id: id.clone(),
        title,
        authors: v
            .get("authors")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .filter(|s| !s.is_empty()),
        language: text(v, "language"),
        publication_year: text(v, "publishedDate").and_then(|s| year(&s)),
        isbn: v
            .get("industryIdentifiers")
            .and_then(Value::as_array)
            .and_then(|a| {
                a.iter()
                    .filter_map(|x| x.get("identifier")?.as_str())
                    .find_map(isbn13)
            }),
        source_url: Some(format!("https://books.google.com/books?id={id}")),
        description: description(v),
    })
}
fn matches_query(book: &BookCandidate, query: &str) -> bool {
    if let Some(isbn) = isbn13(query) {
        return book.isbn.as_deref() == Some(&isbn);
    }
    let haystack =
        format!("{} {}", book.title, book.authors.as_deref().unwrap_or("")).to_lowercase();
    query
        .split_whitespace()
        .all(|word| haystack.contains(&word.to_lowercase()))
}
/// ISBN-10/13 checksum and canonical comparison; no provider guesses.
pub fn isbn13(value: &str) -> Option<String> {
    let s: String = value
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    let s = s.to_uppercase();
    if s.len() == 10 {
        let digits: Option<Vec<u32>> = s
            .chars()
            .enumerate()
            .map(|(i, c)| {
                if i == 9 && c == 'X' {
                    Some(10)
                } else {
                    c.to_digit(10)
                }
            })
            .collect();
        let digits = digits?;
        if digits
            .iter()
            .enumerate()
            .map(|(i, n)| (10 - i as u32) * n)
            .sum::<u32>()
            % 11
            != 0
        {
            return None;
        }
        let prefix = format!("978{}", &s[..9]);
        let sum: u32 = prefix
            .chars()
            .enumerate()
            .map(|(i, c)| c.to_digit(10).unwrap() * if i % 2 == 0 { 1 } else { 3 })
            .sum();
        return Some(format!("{prefix}{}", (10 - sum % 10) % 10));
    }
    if s.len() != 13
        || !s.bytes().all(|c| c.is_ascii_digit())
        || !(s.starts_with("978") || s.starts_with("979"))
    {
        return None;
    }
    (s.bytes()
        .enumerate()
        .map(|(i, c)| (c - b'0') as u32 * if i % 2 == 0 { 1 } else { 3 })
        .sum::<u32>()
        % 10
        == 0)
        .then_some(s)
}

// Other editions deliberately use Open Library work relations independently of Google lookup.
fn ol_key(value: &str, kind: &str, suffix: char) -> bool {
    value
        .strip_prefix(&format!("/{kind}/OL"))
        .and_then(|v| v.strip_suffix(suffix))
        .is_some_and(|v| !v.is_empty() && v.bytes().all(|c| c.is_ascii_digit()))
}
impl HttpBookSource {
    fn ol_url(&self, path: &str) -> Result<reqwest::Url, BookSourceError> {
        let mut url = reqwest::Url::parse(&self.openlibrary)
            .map_err(|_| BookSourceError::new(BookProvider::OpenLibrary, Failure::ServiceError))?;
        url.set_path(path);
        url.set_query(None);
        Ok(url)
    }
    async fn lookup_related(
        &self,
        book: &crate::book_store::Book,
    ) -> Result<Vec<RelatedEdition>, BookSourceError> {
        let mut works = Vec::<(String, String)>::new();
        if let Some(reference) = book
            .external_sources
            .iter()
            .find(|r| r.source == "Open Library" && ol_key(&r.source_id, "books", 'M'))
        {
            let data = self
                .json(
                    self.client
                        .get(self.ol_url(&format!("{}.json", reference.source_id))?),
                    BookProvider::OpenLibrary,
                )
                .await?;
            if data.get("key").and_then(Value::as_str) != Some(reference.source_id.as_str()) {
                return Err(BookSourceError::new(
                    BookProvider::OpenLibrary,
                    Failure::InvalidOutput,
                ));
            }
            for w in data
                .get("works")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(key) = w
                    .get("key")
                    .and_then(Value::as_str)
                    .filter(|k| ol_key(k, "works", 'W'))
                {
                    works.push((key.to_owned(), book.title.clone()));
                }
            }
        } else {
            let mut request = self
                .client
                .get(&self.openlibrary)
                .query(&[("limit", "3"), ("fields", "key,title")]);
            if let Some(isbn) = &book.isbn {
                request = request.query(&[("isbn", isbn)]);
            } else {
                request = request.query(&[("title", &book.title)]);
                if let Some(author) = &book.authors {
                    request = request.query(&[("author", author)]);
                }
            }
            let data = self.json(request, BookProvider::OpenLibrary).await?;
            for w in data
                .get("docs")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    BookSourceError::new(BookProvider::OpenLibrary, Failure::InvalidOutput)
                })?
                .iter()
                .take(3)
            {
                if let Some(raw) = w.get("key").and_then(Value::as_str) {
                    let key = if raw.starts_with("OL") {
                        format!("/works/{raw}")
                    } else {
                        raw.to_owned()
                    };
                    if ol_key(&key, "works", 'W') {
                        works.push((key, text(w, "title").unwrap_or_else(|| book.title.clone())));
                    }
                }
            }
        }
        let mut seen = std::collections::HashSet::new();
        let mut seen_works = std::collections::HashSet::new();
        let mut groups = std::collections::BTreeMap::<String, Vec<RelatedEdition>>::new();
        for (key, title) in works
            .into_iter()
            .filter(|(k, _)| seen_works.insert(k.clone()))
            .take(3)
        {
            let data = self
                .json(
                    self.client
                        .get(self.ol_url(&format!("{key}/editions.json"))?)
                        .query(&[("limit", "100")]),
                    BookProvider::OpenLibrary,
                )
                .await?;
            for entry in data
                .get("entries")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    BookSourceError::new(BookProvider::OpenLibrary, Failure::InvalidOutput)
                })?
                .iter()
                .take(100)
            {
                let Some(id) = entry
                    .get("key")
                    .and_then(Value::as_str)
                    .filter(|v| ol_key(v, "books", 'M'))
                else {
                    continue;
                };
                if !seen.insert(id.to_owned()) {
                    continue;
                }
                if let Some(relations) = entry.get("works").and_then(Value::as_array)
                    && !relations
                        .iter()
                        .any(|w| w.get("key").and_then(Value::as_str) == Some(key.as_str()))
                {
                    continue;
                }
                let Some(edition_title) = text(entry, "title") else {
                    continue;
                };
                let candidate = BookCandidate {
                    source: "Open Library".into(),
                    source_id: id.to_owned(),
                    title: edition_title,
                    authors: None,
                    language: entry
                        .pointer("/languages/0/key")
                        .and_then(Value::as_str)
                        .and_then(|s| s.strip_prefix("/languages/"))
                        .map(str::to_owned),
                    publication_year: text(entry, "publish_date").and_then(|s| year(&s)),
                    isbn: entry
                        .get("isbn_13")
                        .or_else(|| entry.get("isbn_10"))
                        .and_then(Value::as_array)
                        .and_then(|a| a.iter().find_map(|v| v.as_str().and_then(isbn13))),
                    source_url: Some(format!("https://openlibrary.org{id}")),
                    description: description(entry),
                };
                if book
                    .external_sources
                    .iter()
                    .any(|r| r.source == candidate.source && r.source_id == candidate.source_id)
                    || candidate
                        .isbn
                        .as_ref()
                        .is_some_and(|i| book.isbn.as_ref() == Some(i))
                {
                    continue;
                }
                groups
                    .entry(candidate.language.clone().unwrap_or_default())
                    .or_default()
                    .push(RelatedEdition {
                        work_id: key.clone(),
                        work_title: title.clone(),
                        book: candidate,
                    });
            }
        }
        // Interleave languages so a work's numerous English editions do not hide other translations.
        let mut out = Vec::new();
        let mut groups: Vec<_> = groups.into_values().map(|v| v.into_iter()).collect();
        while out.len() < 20 {
            let previous = out.len();
            for group in &mut groups {
                if let Some(v) = group.next() {
                    out.push(v);
                    if out.len() == 20 {
                        break;
                    }
                }
            }
            if out.len() == previous {
                break;
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, http::StatusCode, routing::get};
    use serde_json::json;
    use std::io;
    use tracing_subscriber::fmt::MakeWriter;

    async fn mock_server(
        google_status: StatusCode,
        google_body: String,
        ol_status: StatusCode,
        ol_body: String,
        hang_google: bool,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let app = Router::new()
            .route(
                "/volumes",
                get(move || {
                    let body = google_body.clone();
                    async move {
                        if hang_google {
                            std::future::pending::<()>().await;
                        }
                        (google_status, body)
                    }
                }),
            )
            .route(
                "/search.json",
                get(move || {
                    let body = ol_body.clone();
                    async move { (ol_status, body) }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (url, handle)
    }

    #[tokio::test]
    async fn classifies_google_books_and_open_library_failures() {
        let cases = [
            (
                StatusCode::FORBIDDEN,
                json!({"error":{"code":403,"message":"Permission denied on the service.","details":[{"reason":"ACCESS_DENIED"}]}}).to_string(),
                BookSourceError::http(BookProvider::GoogleBooks, 403, Failure::InvalidKey),
            ),
            (
                StatusCode::FORBIDDEN,
                json!({"error":{"code":403,"message":"Daily Limit Exceeded","errors":[{"reason":"dailyLimitExceeded"}]}}).to_string(),
                BookSourceError::http(BookProvider::GoogleBooks, 403, Failure::QuotaExceeded),
            ),
            (
                StatusCode::BAD_REQUEST,
                json!({"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","details":[{"reason":"API_KEY_INVALID"}]}}).to_string(),
                BookSourceError::http(BookProvider::GoogleBooks, 400, Failure::InvalidKey),
            ),
            (
                StatusCode::TOO_MANY_REQUESTS,
                json!({"error":{"code":429,"message":"Too many requests"}}).to_string(),
                BookSourceError::http(BookProvider::GoogleBooks, 429, Failure::RateLimited),
            ),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "upstream error".to_owned(),
                BookSourceError::http(BookProvider::GoogleBooks, 500, Failure::ServiceError),
            ),
            (
                StatusCode::OK,
                "{not-json".to_owned(),
                BookSourceError::new(BookProvider::GoogleBooks, Failure::InvalidOutput),
            ),
        ];

        for (status, body, expected) in cases {
            let (url, server) = mock_server(
                status,
                body,
                StatusCode::OK,
                json!({"docs":[]}).to_string(),
                false,
            )
            .await;
            let src = HttpBookSource::with_urls(
                Some("synthetic-key".into()),
                format!("{url}/volumes"),
                format!("{url}/search.json"),
            );
            let err = src.search("ethics").await.unwrap_err();
            assert_eq!(err, expected);
            server.abort();
        }

        // Timeout without fixed sleep
        let (url, server) = mock_server(
            StatusCode::OK,
            "{}".to_owned(),
            StatusCode::OK,
            "{}".to_owned(),
            true,
        )
        .await;
        let src = HttpBookSource::with_urls(
            Some("synthetic-key".into()),
            format!("{url}/volumes"),
            format!("{url}/search.json"),
        )
        .with_provider_timeout(Duration::from_millis(50));
        let err = src.search("ethics").await.unwrap_err();
        assert_eq!(
            err,
            BookSourceError::new(BookProvider::GoogleBooks, Failure::Timeout)
        );
        server.abort();

        // Open Library 503 when no key is set
        let (url, server) = mock_server(
            StatusCode::OK,
            "{}".to_owned(),
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable".to_owned(),
            false,
        )
        .await;
        let src =
            HttpBookSource::with_urls(None, format!("{url}/volumes"), format!("{url}/search.json"));
        let err = src.search("ethics").await.unwrap_err();
        assert_eq!(
            err,
            BookSourceError::http(BookProvider::OpenLibrary, 503, Failure::ServiceError)
        );
        server.abort();
    }

    #[derive(Clone, Default)]
    struct Captured(Arc<std::sync::Mutex<Vec<u8>>>);
    struct Sink(Arc<std::sync::Mutex<Vec<u8>>>);
    impl io::Write for Sink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl<'a> MakeWriter<'a> for Captured {
        type Writer = Sink;
        fn make_writer(&'a self) -> Sink {
            Sink(self.0.clone())
        }
    }

    #[tokio::test]
    async fn warn_logs_omit_api_key_query_and_upstream_body() {
        let captured = Captured::default();
        let subscriber = crate::logging::json_subscriber()
            .with_env_filter(crate::logging::env_filter(Some("warn")))
            .with_writer(captured.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let (url, server) = mock_server(
            StatusCode::FORBIDDEN,
            json!({"error":{"code":403,"message":"secret-body-marker"}}).to_string(),
            StatusCode::OK,
            "{}".to_owned(),
            false,
        )
        .await;
        let src = HttpBookSource::with_urls(
            Some("AIza-synthetic-secret-key".into()),
            format!("{url}/volumes"),
            format!("{url}/search.json"),
        );
        let err = src.search("secret-query-text").await.unwrap_err();
        assert_eq!(
            err,
            BookSourceError::http(BookProvider::GoogleBooks, 403, Failure::InvalidKey)
        );
        server.abort();

        let logs = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        assert!(logs.contains("google_books"), "{logs}");
        assert!(logs.contains("invalid_key"), "{logs}");
        assert!(logs.contains("403"), "{logs}");
        assert!(!logs.contains("AIza-synthetic-secret-key"), "{logs}");
        assert!(!logs.contains("secret-query-text"), "{logs}");
        assert!(!logs.contains("secret-body-marker"), "{logs}");
    }
}
