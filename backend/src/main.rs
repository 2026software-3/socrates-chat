use std::sync::Arc;

use socrates_chat_backend::{
    AppState, ai::OpenAiProvider, app, auth_store, catalog, config::Config, demo,
    identity::GoogleIdentity, logging, password, summary,
};
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    logging::json_subscriber()
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
    // 重啟前進行中的總結不會繼續產生：標記為失敗，讓學生可以重試
    match summary::recover_orphaned(&pool).await {
        Ok(0) => {}
        Ok(n) => tracing::warn!(count = n, "marked orphaned summaries as failed"),
        Err(_) => tracing::error!("failed to recover orphaned summaries"),
    }
    // 內建題目（電車難題）：已存在時不動它
    if catalog::seed_builtin_topics(&pool).await.is_err() {
        tracing::error!("failed to seed built-in topics");
    }
    // 首位管理者的內建帳號：初始密碼來自環境變數，已存在的帳號不會被重設（S-01.3）
    if let Some(initial) = &config.admin_initial_password {
        let hash = password::hash(initial).expect("failed to hash ADMIN_INITIAL_PASSWORD");
        match auth_store::bootstrap_admins(&pool, &config.admin_emails, &hash).await {
            Ok(n) => tracing::info!(created = n, "bootstrapped admin accounts"),
            Err(_) => tracing::error!("failed to bootstrap admin accounts"),
        }
    }
    // 示範模式：固定密碼的示範教師與學生（不含管理者）；每次啟動都重設回公開的密碼
    if config.demo_mode {
        match demo::seed_accounts(&pool).await {
            Ok(()) => tracing::warn!("DEMO_MODE is on: public demo accounts are active"),
            Err(_) => tracing::error!("failed to seed demo accounts"),
        }
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
        config.openai_keys.clone(),
        &config.openai_model,
    )
    .with_fallbacks(config.openai_fallback_models.clone());
    let state = AppState::new(pool, config, Arc::new(identity), Arc::new(ai));
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind listener");
    tracing::info!("listening on {addr}");

    // ConnectInfo 提供 TCP 對端位址，登入的 IP 限流要用（S-08.2）
    axum::serve(
        listener,
        app(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .expect("server error");
}
