//! Development-team command: DATABASE_URL=... cargo run --bin import-books -- books.csv
use socrates_chat_backend::{
    book_import::{ImportError, import_csv},
    book_sources::HttpBookSource,
    config::{parse_database_max_connections_or, resolve_google_api_key},
};

/// 沒設定 `DATABASE_MAX_CONNECTIONS` 時一次性匯入只用 3 條，不和正在服務的後端搶連線。
const DEFAULT_IMPORT_MAX_CONNECTIONS: u32 = 3;

#[tokio::main]
async fn main() {
    if let Err(message) = run().await {
        eprintln!("{message}");
        std::process::exit(1)
    }
}
async fn run() -> Result<(), &'static str> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.as_slice() == ["--help"] || args.as_slice() == ["-h"] {
        println!(
            "Usage: import-books <books.csv>\nRequires DATABASE_URL. DATABASE_MAX_CONNECTIONS (1-100, default 3) sets the pool size. GOOGLE_API_KEY (or GOOGLE_BOOKS_API_KEY) enables optional metadata lookup.\nOnly creates shared books; no topic/activity associations. Results are JSON on stdout."
        );
        return Ok(());
    }
    if args.len() != 1 {
        return Err("Usage: import-books <books.csv>");
    }
    let text = std::fs::read_to_string(&args[0]).map_err(|_| "Cannot read UTF-8 CSV file")?;
    let url = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required")?;
    let max_connections = parse_database_max_connections_or(
        std::env::var("DATABASE_MAX_CONNECTIONS").ok().as_deref(),
        DEFAULT_IMPORT_MAX_CONNECTIONS,
    )
    .map_err(|_| "DATABASE_MAX_CONNECTIONS must be a whole number between 1 and 100")?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(&url)
        .await
        .map_err(|_| "Database connection failed")?;
    let source = HttpBookSource::new(resolve_google_api_key(
        std::env::var("GOOGLE_BOOKS_API_KEY").ok().as_deref(),
        std::env::var("GOOGLE_API_KEY").ok().as_deref(),
    ));
    let result=import_csv(&pool,&text,&source).await.map_err(|e|match e {ImportError::Busy=>"已有匯入作業進行中，請稍後手動重試",ImportError::InvalidHeader=>"Invalid CSV header: title required; use documented columns without duplicates",ImportError::Database(_)=>"Database import failed; earlier completed rows remain saved. Check migration and retry."})?;
    println!(
        "{}",
        serde_json::to_string_pretty(&result).map_err(|_| "Cannot encode import report")?
    );
    Ok(())
}
