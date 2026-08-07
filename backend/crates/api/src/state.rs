use ght_core::config::Settings;
use sqlx::PgPool;

use crate::rate_limit::DiscoverRateLimiters;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub settings: Settings,
    pub discover_rate_limiters: DiscoverRateLimiters,
}

impl AppState {
    pub fn new(pool: PgPool, settings: Settings) -> Self {
        let discover_rate_limiters = DiscoverRateLimiters::from_settings(&settings);
        Self {
            pool,
            settings,
            discover_rate_limiters,
        }
    }
}
