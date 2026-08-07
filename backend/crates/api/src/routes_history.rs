use crate::auth::extract::RequireAuth;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Json, Router};
use chrono::{Duration, NaiveDate, Utc};
use ght_core::models::Board;
use ght_core::store as store;
use serde::{Deserialize, Serialize};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/snapshot-dates", axum::routing::get(snapshot_dates))
        .route("/api/repo/history", axum::routing::get(repo_history))
}

#[derive(Deserialize)]
struct SnapshotDatesParams {
    /// board: top_stars | top_forks | top_watchers | trending_daily
    pub board: Option<String>,
}

#[derive(Deserialize)]
struct HistoryParams {
    pub full_name: String,
    /// board: top_stars | top_forks | top_watchers | trending_daily
    pub board: Option<String>,
    /// look-back window in days (default 30, max 365)
    pub days: Option<i64>,
}

#[derive(Serialize)]
struct HistoryPointDto {
    date: String,
    stars: i32,
    forks: i32,
    watchers: Option<i32>,
    stars_today: Option<i32>,
}

fn parse_board(s: Option<&str>) -> Board {
    s.and_then(Board::parse).unwrap_or(Board::TopStars)
}

async fn snapshot_dates(
    State(state): State<AppState>,
    _auth: RequireAuth,
    Query(params): Query<SnapshotDatesParams>,
) -> impl IntoResponse {
    let board = parse_board(params.board.as_deref());
    match store::list_snapshot_dates(&state.pool, board).await {
        Ok(dates) => Json(serde_json::json!({
            "board": board.as_str(),
            "dates": dates.iter().map(|d| d.format("%Y-%m-%d").to_string()).collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn repo_history(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
    Query(params): Query<HistoryParams>,
) -> impl IntoResponse {
    if params.full_name.is_empty() || !params.full_name.contains('/') {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "full_name required as owner/name"})),
        )
            .into_response();
    }
    let board = parse_board(params.board.as_deref());
    // Personal board: only the user who tracks the repo may read tracked_daily history.
    if board == Board::TrackedDaily {
        let allowed =
            match crate::routes_track::tracked_full_names(&state.pool, claims.sub).await {
                Ok(set) => set.contains(&params.full_name),
                Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            };
        if !allowed {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": "tracked_daily history only for own tracked repos"})),
            )
                .into_response();
        }
    }
    let days = params.days.unwrap_or(30).clamp(1, 365);
    let to = Utc::now().date_naive();
    let from = to - Duration::days(days);
    match store::repo_history(&state.pool, &params.full_name, board, from, to).await {
        Ok(points) => {
            let items: Vec<HistoryPointDto> = points
                .into_iter()
                .map(|p| HistoryPointDto {
                    date: p.snapshot_date.format("%Y-%m-%d").to_string(),
                    stars: p.stars,
                    forks: p.forks,
                    watchers: p.watchers,
                    stars_today: p.stars_today,
                })
                .collect();
            Json(serde_json::json!({
                "full_name": params.full_name,
                "board": board.as_str(),
                "from": from.format("%Y-%m-%d").to_string(),
                "to": to.format("%Y-%m-%d").to_string(),
                "points": items,
            }))
            .into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Resolve optional YYYY-MM-DD date, falling back to latest for board.
pub async fn resolve_date(
    pool: &sqlx::PgPool,
    board: Board,
    date: Option<&str>,
) -> Option<NaiveDate> {
    if let Some(raw) = date {
        if let Ok(d) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
            return Some(d);
        }
        return None;
    }
    store::latest_snapshot_date(pool, board).await.ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::tokens;
    use crate::state::AppState;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use ght_core::models::{Board, RepoInput, SnapshotInput};
    use ght_core::{db, store};
    use serial_test::serial;
    use tower::ServiceExt;

    async fn test_state() -> AppState {
        let url = std::env::var("DATABASE_URL_TEST_API")
            .or_else(|_| std::env::var("DATABASE_URL_TEST"))
            .unwrap_or_else(|_| "postgres://postgres@localhost:5432/ghtrending_test_api".into());
        let pool = db::pg_pool(&url)
            .await
            .expect("test db unreachable; run `make db`");
        db::migrate(&pool).await.unwrap();
        sqlx::query(
            "TRUNCATE repos, snapshots, users, invite_codes, refresh_tokens, user_tracked_repos",
        )
        .execute(&pool)
        .await
        .unwrap();
        let settings = ght_core::config::Settings::from_map(|k| match k {
            "DATABASE_URL" => Some(url.clone()),
            "JWT_SECRET" => Some("test-secret".into()),
            _ => None,
        })
        .unwrap();
        AppState { pool, settings }
    }

    async fn get(state: AppState, uri: &str, cookie: &str) -> (StatusCode, String) {
        let resp = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(uri)
                    .header("cookie", cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    #[tokio::test]
    #[serial]
    async fn history_and_dates_and_date_filter() {
        let state = test_state().await;
        let d1 = NaiveDate::from_ymd_opt(2026, 8, 5).unwrap();
        let d2 = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();
        for (d, stars) in [(d1, 100), (d2, 150)] {
            let id = store::upsert_repo(
                &state.pool,
                &RepoInput {
                    full_name: "hist/repo".into(),
                    owner: "hist".into(),
                    name: "repo".into(),
                    html_url: "https://github.com/hist/repo".into(),
                    language: Some("Rust".into()),
                    description: None,
                    license: None,
                    topics: vec![],
                    languages_json: RepoInput::languages_empty(),
                    language_names: vec![],
                },
                d,
            )
            .await
            .unwrap();
            store::upsert_snapshot(
                &state.pool,
                id,
                d,
                Board::TopStars,
                &SnapshotInput {
                    stars,
                    forks: 1,
                    watchers: None,
                    stars_today: None,
                },
            )
            .await
            .unwrap();
        }

        let cookie = format!(
            "access_token={}",
            tokens::issue_access(&state.settings.jwt_secret, 1, "t").unwrap()
        );

        let (status, body) = get(
            state.clone(),
            "/api/snapshot-dates?board=top_stars",
            &cookie,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let dates = v["dates"].as_array().unwrap();
        assert!(dates.iter().any(|x| x == "2026-08-06"));
        assert!(dates.iter().any(|x| x == "2026-08-05"));

        let (status, body) = get(
            state.clone(),
            "/api/repo/history?full_name=hist/repo&board=top_stars&days=365",
            &cookie,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["points"].as_array().unwrap().len(), 2);
        assert_eq!(v["points"][1]["stars"], 150);

        let (status, body) = get(
            state.clone(),
            "/api/leaderboard/top?metric=stars&date=2026-08-05",
            &cookie,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["date"], "2026-08-05");
        assert_eq!(v["items"][0]["stars"], 100);
    }
}
