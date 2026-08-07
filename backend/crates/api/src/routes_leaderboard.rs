use crate::auth::extract::RequireAuth;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Json, Router};
use ght_core::models::{Board, LeaderboardFilter};
use ght_core::store as store;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct TopParams {
    pub metric: String,
    pub language: Option<String>,
    /// Optional snapshot date YYYY-MM-DD; defaults to latest
    pub date: Option<String>,
}

#[derive(Deserialize)]
pub struct TrendingParams {
    pub language: Option<String>,
    /// Optional snapshot date YYYY-MM-DD; defaults to latest
    pub date: Option<String>,
}

#[derive(Deserialize)]
pub struct LanguageParams {
    /// top_stars | top_forks | top_watchers | trending_daily; defaults to top_stars
    pub board: Option<String>,
    /// Optional snapshot date YYYY-MM-DD; defaults to latest for the board
    pub date: Option<String>,
}

#[derive(Serialize)]
pub struct LeaderboardItem {
    pub rank: i64,
    pub full_name: String,
    pub html_url: String,
    pub description: Option<String>,
    pub language: Option<String>,
    pub stars: i32,
    pub forks: i32,
    pub watchers: Option<i32>,
    pub stars_today: Option<i32>,
}

#[derive(Serialize)]
pub struct LeaderboardResp {
    pub date: String,
    pub board: String,
    pub language: Option<String>,
    pub items: Vec<LeaderboardItem>,
}

fn to_items(rows: Vec<ght_core::models::LeaderboardRow>) -> Vec<LeaderboardItem> {
    rows.into_iter()
        .map(|r| LeaderboardItem {
            rank: r.rank,
            full_name: r.full_name,
            html_url: r.html_url,
            description: r.description,
            language: r.language,
            stars: r.stars,
            forks: r.forks,
            watchers: r.watchers,
            stars_today: r.stars_today,
        })
        .collect()
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/leaderboard/top", axum::routing::get(top))
        .route("/api/leaderboard/trending", axum::routing::get(trending))
        .route("/api/languages", axum::routing::get(languages))
        .route("/api/meta", axum::routing::get(meta))
        .route("/api/health", axum::routing::get(health))
        .route("/api/ready", axum::routing::get(ready))
}

async fn top(
    State(state): State<AppState>,
    _auth: RequireAuth,
    Query(params): Query<TopParams>,
) -> impl IntoResponse {
    let board = match params.metric.as_str() {
        "stars" => Board::TopStars,
        "forks" => Board::TopForks,
        "watchers" => Board::TopWatchers,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error":"metric must be stars|forks|watchers"})),
            )
                .into_response()
        }
    };
    let date = match crate::routes_history::resolve_date(
        &state.pool,
        board,
        params.date.as_deref(),
    )
    .await
    {
        Some(d) => d,
        None if params.date.is_some() => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error":"invalid date, expected YYYY-MM-DD"})),
            )
                .into_response()
        }
        None => {
            return Json(LeaderboardResp {
                date: String::new(),
                board: board.as_str().to_string(),
                language: params.language.clone(),
                items: vec![],
            })
            .into_response()
        }
    };
    let filter = LeaderboardFilter {
        language: params.language.as_deref(),
        ..LeaderboardFilter::empty()
    };
    let rows = match board {
        Board::TopStars => store::top_by_stars(&state.pool, date, filter, 100).await,
        Board::TopForks => store::top_by_forks(&state.pool, date, filter, 100).await,
        Board::TopWatchers => store::top_by_watchers(&state.pool, date, filter, 100).await,
        _ => unreachable!(),
    };
    match rows {
        Ok(rows) => Json(LeaderboardResp {
            date: date.format("%Y-%m-%d").to_string(),
            board: board.as_str().to_string(),
            language: params.language.clone(),
            items: to_items(rows),
        })
        .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn trending(
    State(state): State<AppState>,
    _auth: RequireAuth,
    Query(params): Query<TrendingParams>,
) -> impl IntoResponse {
    let board = Board::TrendingDaily;
    let date = match crate::routes_history::resolve_date(
        &state.pool,
        board,
        params.date.as_deref(),
    )
    .await
    {
        Some(d) => d,
        None if params.date.is_some() => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error":"invalid date, expected YYYY-MM-DD"})),
            )
                .into_response()
        }
        None => {
            return Json(LeaderboardResp {
                date: String::new(),
                board: board.as_str().to_string(),
                language: params.language.clone(),
                items: vec![],
            })
            .into_response()
        }
    };
    let filter = LeaderboardFilter {
        language: params.language.as_deref(),
        ..LeaderboardFilter::empty()
    };
    match store::trending(&state.pool, date, filter, 100).await {
        Ok(rows) => Json(LeaderboardResp {
            date: date.format("%Y-%m-%d").to_string(),
            board: board.as_str().to_string(),
            language: params.language.clone(),
            items: to_items(rows),
        })
        .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn languages(
    State(state): State<AppState>,
    _auth: RequireAuth,
    Query(params): Query<LanguageParams>,
) -> impl IntoResponse {
    let board = match params.board.as_deref() {
        Some(b) => match Board::parse(b) {
            Some(b) => b,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error":"board must be top_stars|top_forks|top_watchers|trending_daily"})),
                )
                    .into_response()
            }
        },
        None => Board::TopStars,
    };
    let date =
        match crate::routes_history::resolve_date(&state.pool, board, params.date.as_deref()).await
        {
            Some(d) => d,
            None if params.date.is_some() => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error":"invalid date, expected YYYY-MM-DD"})),
                )
                    .into_response()
            }
            None => return Json(serde_json::json!([])).into_response(),
        };
    match store::languages_with_counts(&state.pool, date, board).await {
        Ok(rows) => {
            let items: Vec<serde_json::Value> = rows
                .into_iter()
                .map(|(language, count)| serde_json::json!({ "language": language, "count": count }))
                .collect();
            Json(serde_json::json!(items)).into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn meta(State(state): State<AppState>, _auth: RequireAuth) -> impl IntoResponse {
    let date = store::latest_snapshot_date(&state.pool, Board::TopStars)
        .await
        .ok()
        .flatten();
    let mut counts = serde_json::Map::new();
    if let Some(d) = date {
        for board in [
            Board::TopStars,
            Board::TopForks,
            Board::TopWatchers,
            Board::TrendingDaily,
        ] {
            let n = store::board_count(&state.pool, d, board).await.unwrap_or(0);
            counts.insert(board.as_str().to_string(), serde_json::json!(n));
        }
    }
    let dates = store::list_snapshot_dates(&state.pool, Board::TopStars)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|d| d.format("%Y-%m-%d").to_string())
        .collect::<Vec<_>>();
    Json(serde_json::json!({
        "date": date.map(|d| d.format("%Y-%m-%d").to_string()),
        "boards": counts,
        "dates": dates,
    }))
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

/// Readiness: process up + database reachable (for k8s readinessProbe).
async fn ready(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
    {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "status": "ready" }))).into_response(),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "status": "not_ready", "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use chrono::NaiveDate;
    use ght_core::models::{Board, RepoInput, SnapshotInput};
    use ght_core::store as store;
    use ght_core::db;
    use serial_test::serial;
    use tower::ServiceExt;

    use crate::auth::tokens;
    use crate::state::AppState;

    async fn test_state() -> AppState {
        let url = std::env::var("DATABASE_URL_TEST_API")
            .or_else(|_| std::env::var("DATABASE_URL_TEST"))
            .unwrap_or_else(|_| "postgres://ght:ght@localhost:5433/ghtrending_test_api".into());
        let pool = db::pg_pool(&url)
            .await
            .expect("test db unreachable; run `make db`");
        db::migrate(&pool).await.unwrap();
        sqlx::query("TRUNCATE repos, snapshots, users, invite_codes, refresh_tokens")
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

    fn repo(full_name: &str, lang: Option<&str>) -> RepoInput {
        let (owner, name) = full_name.split_once('/').unwrap();
        RepoInput {
            full_name: full_name.into(),
            owner: owner.into(),
            name: name.into(),
            html_url: format!("https://github.com/{full_name}"),
            language: lang.map(String::from),
            description: None,
            topics: vec![],
            languages_json: RepoInput::languages_empty(),
            language_names: vec![],
        }
    }

    async fn seed(state: &AppState) {
        let date = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();
        for (name, lang, stars, forks, watchers) in [
            ("a/py1", Some("Python"), 300, 30, 3),
            ("a/py2", Some("Python"), 100, 50, 9),
            ("a/rs1", Some("Rust"), 200, 10, 1),
        ] {
            let id = store::upsert_repo(&state.pool, &repo(name, lang), date)
                .await
                .unwrap();
            store::upsert_snapshot(
                &state.pool,
                id,
                date,
                Board::TopStars,
                &SnapshotInput {
                    stars,
                    forks,
                    watchers: Some(watchers),
                    stars_today: None,
                },
            )
            .await
            .unwrap();
            store::upsert_snapshot(
                &state.pool,
                id,
                date,
                Board::TopForks,
                &SnapshotInput {
                    stars,
                    forks,
                    watchers: Some(watchers),
                    stars_today: None,
                },
            )
            .await
            .unwrap();
            store::upsert_snapshot(
                &state.pool,
                id,
                date,
                Board::TopWatchers,
                &SnapshotInput {
                    stars,
                    forks,
                    watchers: Some(watchers),
                    stars_today: None,
                },
            )
            .await
            .unwrap();
            store::upsert_snapshot(
                &state.pool,
                id,
                date,
                Board::TrendingDaily,
                &SnapshotInput {
                    stars: stars / 10,
                    forks,
                    watchers: None,
                    stars_today: Some(stars / 10),
                },
            )
            .await
            .unwrap();
        }
    }

    fn auth_cookie(state: &AppState) -> String {
        let jwt = tokens::issue_access(&state.settings.jwt_secret, 1, "tester").unwrap();
        format!("access_token={jwt}")
    }

    async fn get(state: AppState, uri: &str, cookie: Option<String>) -> (StatusCode, String) {
        let mut builder = Request::builder().method("GET").uri(uri);
        if let Some(c) = cookie {
            builder = builder.header("cookie", c);
        }
        let resp = crate::build_router(state)
            .oneshot(builder.body(Body::empty()).unwrap())
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
    async fn top_by_stars_and_language_filter() {
        let state = test_state().await;
        seed(&state).await;
        let cookie = auth_cookie(&state);

        let (status, body) =
            get(state.clone(), "/api/leaderboard/top?metric=stars", Some(cookie.clone())).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"full_name\":\"a/py1\""));
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["items"][0]["rank"], 1);
        assert_eq!(v["items"][0]["full_name"], "a/py1");

        let (_, body) = get(
            state.clone(),
            "/api/leaderboard/top?metric=stars&language=Rust",
            Some(cookie.clone()),
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 1);
        assert_eq!(v["items"][0]["full_name"], "a/rs1");
        assert_eq!(v["items"][0]["rank"], 1);
    }

    #[tokio::test]
    #[serial]
    async fn top_requires_auth() {
        let state = test_state().await;
        seed(&state).await;
        let (status, _) = get(state.clone(), "/api/leaderboard/top?metric=stars", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    #[serial]
    async fn trending_returns_stars_today() {
        let state = test_state().await;
        seed(&state).await;
        let cookie = auth_cookie(&state);
        let (status, body) = get(state.clone(), "/api/leaderboard/trending", Some(cookie)).await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["items"][0]["full_name"], "a/py1");
        assert_eq!(v["items"][0]["stars_today"], 30);
    }

    #[tokio::test]
    #[serial]
    async fn languages_and_meta_and_health() {
        let state = test_state().await;
        seed(&state).await;
        let cookie = auth_cookie(&state);

        let (_, body) = get(state.clone(), "/api/languages", Some(cookie.clone())).await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let langs = v.as_array().unwrap();
        let names: Vec<&str> = langs.iter().map(|x| x["language"].as_str().unwrap()).collect();
        assert!(names.contains(&"Python") && names.contains(&"Rust"));
        // Regression for cross-board double counting: seed puts 2 Python repos
        // on ALL four boards, so the top_stars count must be 2, not 8.
        let py = langs.iter().find(|x| x["language"] == "Python").unwrap();
        assert_eq!(py["count"], 2);
        // The advertised count must equal the number of items the
        // language-filtered leaderboard returns.
        let (_, body) = get(
            state.clone(),
            "/api/leaderboard/top?metric=stars&language=Python",
            Some(cookie.clone()),
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            v["items"].as_array().unwrap().len(),
            py["count"].as_i64().unwrap() as usize
        );
        // Per-board language counts.
        let (status, body) =
            get(state.clone(), "/api/languages?board=top_forks", Some(cookie.clone())).await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let py = v
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["language"] == "Python")
            .cloned()
            .unwrap();
        assert_eq!(py["count"], 2);
        // Invalid board is rejected.
        let (status, _) =
            get(state.clone(), "/api/languages?board=bogus", Some(cookie.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (_, body) = get(state.clone(), "/api/meta", Some(cookie.clone())).await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["date"], "2026-08-06");
        assert_eq!(v["boards"]["top_stars"], 3);
        assert_eq!(v["boards"]["trending_daily"], 3);

        let (status, _) = get(state.clone(), "/api/health", None).await;
        assert_eq!(status, StatusCode::OK);

        let (status, body) = get(state.clone(), "/api/ready", None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("ready"));
    }
}
