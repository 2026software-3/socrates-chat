use std::sync::Arc;

use socrates_chat_backend::{
    AppState, ai::OpenAiProvider, app, config::Config, identity::GoogleIdentity,
};
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = Config::from_env().expect("invalid configuration");
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await
        .expect("failed to connect to database");
    // 正式環境的 migration 於部署前單獨執行（S-09.3）；本機開發可設 RUN_MIGRATIONS=true。
    if std::env::var("RUN_MIGRATIONS").is_ok_and(|v| v == "true") {
        sqlx::migrate!()
            .run(&pool)
            .await
            .expect("failed to run migrations");
    }
    let identity = GoogleIdentity::discover(
        &config.google_client_id,
        &config.google_client_secret,
        &config.google_redirect_url,
    )
    .await
    .expect("failed to discover Google OIDC configuration");

    let addr = config.listen_addr.clone();
    let ai = OpenAiProvider::new(
        &config.openai_base_url,
        &config.openai_api_key,
        &config.openai_model,
    );
    let state = AppState::new(pool, config, Arc::new(identity), Arc::new(ai));
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind listener");
    tracing::info!("listening on {addr}");

    axum::serve(listener, app(state))
        .await
        .expect("server error");
}
