use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

pub async fn pg_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new().max_connections(8).connect(database_url).await
}
