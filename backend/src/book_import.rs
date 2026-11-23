//! Developer-only CSV import; no upload endpoint and no implicit topic associations.
#[derive(Debug, PartialEq)]
pub struct CsvRow {
    pub line: usize,
    pub fields: Result<Vec<String>, &'static str>,
}
pub fn parse_csv(text: &str) -> Vec<CsvRow> {
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    let (mut line, mut start) = (1, 1);
    let (mut quoted, mut closed) = (false, false);
    let mut field = String::new();
    let mut fields = Vec::new();
    let mut rows = Vec::new();
    let mut error = None;
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                if c == '\n' {
                    line += 1;
                }
                field.push(c);
            }
        } else if c == ',' {
            fields.push(std::mem::take(&mut field));
            closed = false;
        } else if c == '\n' || c == '\r' {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            if !fields.is_empty() || !field.is_empty() || closed || error.is_some() {
                fields.push(std::mem::take(&mut field));
                rows.push(CsvRow {
                    line: start,
                    fields: match error.take() {
                        Some(e) => Err(e),
                        None => Ok(std::mem::take(&mut fields)),
                    },
                });
            }
            fields.clear();
            field.clear();
            closed = false;
            line += 1;
            start = line;
        } else if c == '"' && field.is_empty() && !closed {
            quoted = true;
        } else {
            if c == '"' || closed {
                error = Some("invalid_csv");
            }
            field.push(c);
        }
    }
    if quoted {
        error = Some("unclosed_quote");
    }
    if !fields.is_empty() || !field.is_empty() || closed || error.is_some() {
        fields.push(field);
        rows.push(CsvRow {
            line: start,
            fields: match error {
                Some(e) => Err(e),
                None => Ok(fields),
            },
        });
    }
    rows
}

use crate::book_sources::BookSource;
use serde::Serialize;
use sqlx::PgPool;

#[derive(Debug, Default, Serialize)]
pub struct ImportReport {
    pub added: usize,
    pub skipped: usize,
    pub failed: usize,
    pub rows: Vec<ImportRow>,
}
#[derive(Debug, Serialize)]
pub struct ImportRow {
    pub line: usize,
    pub title: String,
    pub status: &'static str,
    pub reason: Option<String>,
}
#[derive(Debug)]
pub enum ImportError {
    Busy,
    InvalidHeader,
    Database(sqlx::Error),
}
impl From<sqlx::Error> for ImportError {
    fn from(e: sqlx::Error) -> Self {
        Self::Database(e)
    }
}
pub const IMPORT_LOCK: i64 = 831416;
fn normalized(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}
fn author_key(value: &str) -> Vec<String> {
    let mut names: Vec<_> = value
        .split(';')
        .map(normalized)
        .filter(|s| !s.is_empty())
        .collect();
    names.sort();
    names.dedup();
    names
}

pub async fn import_csv(
    pool: &PgPool,
    text: &str,
    source: &dyn BookSource,
) -> Result<ImportReport, ImportError> {
    use crate::{
        book_store::{self, BookData},
        books::normalize_book,
    };
    let mut rows = parse_csv(text).into_iter();
    let headers = rows
        .next()
        .ok_or(ImportError::InvalidHeader)?
        .fields
        .map_err(|_| ImportError::InvalidHeader)?;
    let known = [
        "title",
        "authors",
        "isbn",
        "language",
        "publication_year",
        "source_url",
        "description",
    ];
    if !headers.iter().any(|h| h == "title")
        || headers.iter().any(|h| !known.contains(&h.as_str()))
        || headers
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != headers.len()
    {
        return Err(ImportError::InvalidHeader);
    }
    // Held on its own connection for the job; row commits remain independent.
    // Transaction-scoped advisory lock also releases on cancellation/disconnection.
    let mut job = pool.begin().await?;
    let (locked,): (bool,) = sqlx::query_as("SELECT pg_try_advisory_xact_lock($1)")
        .bind(IMPORT_LOCK)
        .fetch_one(&mut *job)
        .await?;
    if !locked {
        return Err(ImportError::Busy);
    }
    let mut report = ImportReport::default();
    for row in rows {
        let line = row.line;
        let fields = match row.fields {
            Ok(f) if f.len() == headers.len() => f,
            _ => {
                report.failed += 1;
                report.rows.push(ImportRow {
                    line,
                    title: String::new(),
                    status: "failed",
                    reason: Some("invalid_csv_row".into()),
                });
                continue;
            }
        };
        let get = |name: &str| {
            headers
                .iter()
                .position(|h| h == name)
                .map(|i| fields[i].clone())
        };
        let raw = BookData {
            title: get("title").unwrap_or_default(),
            authors: get("authors"),
            isbn: get("isbn"),
            language: get("language"),
            publication_year: get("publication_year"),
            source_url: get("source_url"),
            description: get("description"),
            recommendation_note: None,
            external_sources: vec![],
        };
        let original = raw.title.clone();
        let mut data = match normalize_book(raw) {
            Ok(v) => v,
            Err(e) => {
                report.failed += 1;
                report.rows.push(ImportRow {
                    line,
                    title: original,
                    status: "failed",
                    reason: Some(e.code.into()),
                });
                continue;
            }
        };
        let mut pending = None;
        match source.search(&data.title).await {
            Ok(candidates) if candidates.len() == 1 => {
                let candidate = &candidates[0];
                let version_conflict = data
                    .isbn
                    .as_ref()
                    .zip(candidate.isbn.as_ref())
                    .is_some_and(|(a, b)| a != b)
                    || data
                        .authors
                        .as_ref()
                        .zip(candidate.authors.as_ref())
                        .is_some_and(|(a, b)| author_key(a) != author_key(b))
                    || data
                        .language
                        .as_ref()
                        .zip(candidate.language.as_ref())
                        .is_some_and(|(a, b)| {
                            !crate::books::canonical_book_language(a)
                                .eq_ignore_ascii_case(&crate::books::canonical_book_language(b))
                        })
                    || data
                        .publication_year
                        .as_ref()
                        .zip(candidate.publication_year.as_ref())
                        .is_some_and(|(a, b)| normalized(a) != normalized(b));
                if version_conflict {
                    pending = Some("version_conflict".into());
                } else if data.isbn.is_some() && candidate.isbn.is_none() {
                    pending = Some("version_unconfirmed".into());
                } else {
                    // Preserve original values; each supplied field is independently validated.
                    let mut adopted = false;
                    macro_rules! fill {
                        ($field:ident) => {
                            if data.$field.is_none() {
                                if let Some(v) = &candidate.$field {
                                    let mut filled = data.clone();
                                    filled.$field = Some(v.clone());
                                    match normalize_book(filled) {
                                        Ok(valid) => {
                                            adopted |= valid.$field.is_some();
                                            data = valid;
                                        }
                                        Err(_) => pending = Some("invalid_supplement".to_owned()),
                                    }
                                }
                            }
                        };
                    }
                    fill!(authors);
                    fill!(isbn);
                    fill!(language);
                    fill!(publication_year);
                    fill!(source_url);
                    fill!(description);
                    if adopted {
                        data.external_sources.push(book_store::BookSourceRef {
                            source: candidate.source.clone(),
                            source_id: candidate.source_id.clone(),
                        });
                    }
                }
            }
            Ok(candidates) if candidates.len() > 1 => pending = Some("multiple_candidates".into()),
            Ok(_) => {}
            Err(_) => pending = Some("source_unavailable".into()),
        }
        let mut tx = pool.begin().await?;
        // Grouping and manual writes use the same lock order; never hold it during lookup.
        book_store::lock_catalog(&mut tx).await?;
        // Short critical section: concurrent manual writes cannot invalidate the duplicate check.
        sqlx::query("LOCK TABLE books IN SHARE ROW EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await?;
        let existing: Vec<(String, Option<String>, Option<String>)> =
            sqlx::query_as("SELECT title,authors,isbn FROM books")
                .fetch_all(&mut *tx)
                .await?;
        let duplicate = existing
            .iter()
            .any(|(title, authors, isbn)| match (&data.isbn, isbn) {
                (Some(a), Some(b)) => a == b,
                _ => {
                    normalized(&data.title) == normalized(title)
                        && (data.authors.is_none()
                            || authors.is_none()
                            || author_key(data.authors.as_deref().unwrap_or(""))
                                == author_key(authors.as_deref().unwrap_or("")))
                }
            });
        if duplicate {
            report.skipped += 1;
            report.rows.push(ImportRow {
                line,
                title: data.title,
                status: "skipped",
                reason: Some("duplicate_or_possible_duplicate".into()),
            });
            tx.rollback().await?;
            continue;
        }
        book_store::create(&mut *tx, None, &data).await?;
        tx.commit().await?;
        report.added += 1;
        report.rows.push(ImportRow {
            line,
            title: data.title,
            status: if pending.is_some() {
                "added_needs_metadata"
            } else {
                "added"
            },
            reason: pending,
        });
    }
    job.rollback().await?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn csv_accepts_bom_quotes_and_multiline_description() {
        let rows = parse_csv(
            "\u{feff}title,description\r\n\"Synthetic, book\",\"line one\nline \"\"two\"\"\"\r\n",
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].line, 2);
        assert_eq!(
            rows[1].fields,
            Ok(vec![
                "Synthetic, book".into(),
                "line one\nline \"two\"".into()
            ])
        );
    }
}
