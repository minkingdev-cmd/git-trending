pub mod auth;
pub mod rate_limit;
pub mod routes_admin;
pub mod routes_discover;
pub mod routes_history;
pub mod routes_leaderboard;
pub mod routes_me_github;
pub mod routes_track;
pub mod state;

use axum::Router;
use state::AppState;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .nest("/api/auth", auth::routes::router())
        .merge(routes_leaderboard::router())
        .merge(routes_history::router())
        .merge(routes_track::router())
        .merge(routes_me_github::router())
        .merge(routes_discover::router())
        .merge(routes_admin::router())
        .with_state(state)
}
