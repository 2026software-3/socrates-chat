use tracing_subscriber::EnvFilter;

// Bind address and other runtime configuration are pending S-08.
const ADDR: &str = "127.0.0.1:3000";

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let listener = tokio::net::TcpListener::bind(ADDR)
        .await
        .expect("failed to bind listener");
    tracing::info!("listening on {ADDR}");

    axum::serve(listener, socrates_chat_backend::app())
        .await
        .expect("server error");
}
