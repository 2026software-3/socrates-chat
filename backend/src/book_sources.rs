//! Teacher book lookup. Credentials stay server-side; provider failures never trigger fallback.
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
#[derive(Debug)]
pub struct BookSourceError;
#[async_trait]
pub trait BookSource: Send + Sync {
    async fn search(&self, query: &str) -> Result<Vec<BookCandidate>, BookSourceError>;
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
        ol: bool,
    ) -> Result<Value, BookSourceError> {
        if ol {
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
            .map_err(|_| BookSourceError)?;
        if !response.status().is_success() {
            return Err(BookSourceError);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| BookSourceError)? {
            if bytes.len() + chunk.len() > 1_000_000 {
                return Err(BookSourceError);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| BookSourceError)
    }
    async fn lookup(&self, query: &str) -> Result<Vec<BookCandidate>, BookSourceError> {
        let key = self
            .key
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or(BookSourceError)?;
        let payload = self
            .json(
                self.client
                    .get(&self.google)
                    .header("X-Goog-Api-Key", key)
                    .query(&[("q", query), ("maxResults", "5"), ("printType", "books")]),
                false,
            )
            .await?;
        // A malformed response is a failure, not a reason to switch providers.
        if !payload.is_object() || payload.get("error").is_some() {
            return Err(BookSourceError);
        }
        let empty = Vec::new();
        let items = match payload.get("items") {
            Some(v) => v.as_array().ok_or(BookSourceError)?,
            None if payload.get("totalItems").and_then(Value::as_u64) == Some(0) => &empty,
            None => return Err(BookSourceError),
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
            .map_err(|_| BookSourceError)?
    }

    async fn lookup_openlibrary(&self, query: &str) -> Result<Vec<BookCandidate>, BookSourceError> {
        let payload=self.json(self.client.get(&self.openlibrary).query(&[("q",query),("limit","5"),("fields","key,title,author_name,editions,editions.key,editions.title,editions.language")]),true).await?;
        let docs = payload
            .get("docs")
            .and_then(Value::as_array)
            .ok_or(BookSourceError)?;
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
            let mut url = reqwest::Url::parse(&self.openlibrary).map_err(|_| BookSourceError)?;
            url.set_path(&format!("/books/{code}.json"));
            url.set_query(None);
            let data = self.json(self.client.get(url), true).await?;
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
#[async_trait]
impl BookSource for HttpBookSource {
    async fn search(&self, query: &str) -> Result<Vec<BookCandidate>, BookSourceError> {
        self.lookup(query.trim()).await
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
