use ght_core::config::Settings;
use ght_core::db;
use tower_http::services::{ServeDir, ServeFile};
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

    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| {
        // Prefer monorepo path when running from repo root or backend/
        if std::path::Path::new("frontend/dist").exists() {
            "frontend/dist".into()
        } else if std::path::Path::new("../frontend/dist").exists() {
            "../frontend/dist".into()
        } else {
            "frontend/dist".into()
        }
    });

    let state = ght_api::state::AppState { pool, settings };
    let index = format!("{static_dir}/index.html");
    let app = ght_api::build_router(state)
        .layer(TraceLayer::new_for_http())
        .fallback_service(
            ServeDir::new(&static_dir).not_found_service(ServeFile::new(index)),
        );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;
    tracing::info!(%static_dir, "api listening on :8000");
    axum::serve(listener, app).await?;
    Ok(())
}
