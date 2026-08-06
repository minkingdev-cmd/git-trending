use ght_core::config::Settings;
use ght_core::db;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let settings = Settings::from_env()?;
    let pool = db::pg_pool(&settings.database_url).await?;
    db::migrate(&pool).await?;

    let state = ght_api::state::AppState { pool, settings };
    let app = ght_api::build_router(state)
        .layer(TraceLayer::new_for_http())
        .fallback_service(ServeDir::new("frontend/dist"));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;
    tracing::info!("api listening on :8000");
    axum::serve(listener, app).await?;
    Ok(())
}
