pub mod auth;
pub mod routes_leaderboard;
pub mod state;

use axum::Router;
use state::AppState;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .nest("/api/auth", auth::routes::router())
        .merge(routes_leaderboard::router())
        .with_state(state)
}
