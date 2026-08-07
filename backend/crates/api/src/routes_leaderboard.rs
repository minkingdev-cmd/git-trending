use crate::auth::extract::RequireAuth;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Json, Router};
use ght_core::models::{Board, LanguageShare, LeaderboardFilter, TopicMode};
use ght_core::store as store;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize)]
pub struct TopParams {
    pub metric: String,
    /// Legacy single primary-language filter (`repos.language` equality).
    pub language: Option<String>,
    /// Comma-separated multi-language OR filter against `language_names`.
    pub languages: Option<String>,
    /// Comma-separated license OR filter (SPDX / short key).
    pub licenses: Option<String>,
    /// Comma-separated topics (lowercase).
    pub topics: Option<String>,
    /// `and` | `or`; default `and`.
    pub topic_mode: Option<String>,
    /// Keyword search over full_name, description, topics, language names, license.
    pub q: Option<String>,
    /// Optional snapshot date YYYY-MM-DD; defaults to latest
    pub date: Option<String>,
}

#[derive(Deserialize)]
pub struct TrendingParams {
    /// Legacy single primary-language filter (`repos.language` equality).
    pub language: Option<String>,
    /// Comma-separated multi-language OR filter against `language_names`.
    pub languages: Option<String>,
    /// Comma-separated license OR filter (SPDX / short key).
    pub licenses: Option<String>,
    /// Comma-separated topics (lowercase).
    pub topics: Option<String>,
    /// `and` | `or`; default `and`.
    pub topic_mode: Option<String>,
    /// Keyword search over full_name, description, topics, language names, license.
    pub q: Option<String>,
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

/// API language share DTO (name + pct; bytes optional).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageShareDto {
    pub name: String,
    pub pct: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<i64>,
}

impl From<LanguageShare> for LanguageShareDto {
    fn from(s: LanguageShare) -> Self {
        Self {
            name: s.name,
            pct: s.pct,
            bytes: s.bytes,
        }
    }
}

#[derive(Serialize)]
pub struct LeaderboardItem {
    pub rank: i64,
    pub full_name: String,
    pub html_url: String,
    pub description: Option<String>,
    /// Deprecated primary language; prefer `languages`.
    pub language: Option<String>,
    /// SPDX id or short license key (MIT, Apache-2.0, …).
    pub license: Option<String>,
    pub topics: Vec<String>,
    pub languages: Vec<LanguageShareDto>,
    pub stars: i32,
    pub forks: i32,
    pub watchers: Option<i32>,
    pub stars_today: Option<i32>,
    /// True when the authenticated user tracks this repo.
    pub tracked_by_me: bool,
}

#[derive(Serialize)]
pub struct TopicFacet {
    pub topic: String,
    pub count: i64,
}

#[derive(Serialize)]
pub struct LanguageFacet {
    pub language: String,
    pub count: i64,
}

#[derive(Serialize)]
pub struct LicenseFacet {
    pub license: String,
    pub count: i64,
}

#[derive(Serialize)]
pub struct LeaderboardResp {
    pub date: String,
    pub board: String,
    /// Echo of legacy single-language query param.
    pub language: Option<String>,
    /// Echo of multi-language filter (comma-split, non-empty).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub languages_filter: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub licenses_filter: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topics_filter: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_mode: Option<String>,
    pub items: Vec<LeaderboardItem>,
    /// v1: result-set unnest counts from filtered `items` (not full disjunctive facets).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_facets: Option<Vec<TopicFacet>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_facets: Option<Vec<LanguageFacet>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license_facets: Option<Vec<LicenseFacet>>,
}

/// Parse comma-separated list; trim, drop empty. For topics, also lowercase.
fn parse_csv_list(raw: Option<&str>, lowercase: bool) -> Vec<String> {
    let Some(s) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return vec![];
    };
    let mut out: Vec<String> = s
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| {
            if lowercase {
                p.to_lowercase()
            } else {
                p.to_string()
            }
        })
        .collect();
    // Preserve order but dedupe while keeping first occurrence.
    let mut seen = std::collections::HashSet::new();
    out.retain(|x| seen.insert(x.clone()));
    out
}

fn parse_topic_mode(raw: Option<&str>) -> Result<TopicMode, ()> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(TopicMode::And),
        Some(s) => TopicMode::parse(&s.to_lowercase()).ok_or(()),
    }
}

/// Build filter from parsed query pieces.
///
/// Backward compat:
/// - Legacy `language` → `LeaderboardFilter.language` (primary equality on
///   `repos.language`). Works even when `language_names` is empty.
/// - Multi-select `languages` → OR on `language_names` **or** primary
///   `repos.language` (so facet chips work before full share enrichment).
/// - When only legacy `language` is present, response still echoes it under
///   `languages_filter`; it binds primary-equality only (not multi-array).
struct ParsedFilters {
    language: Option<String>,
    /// Multi-language filter as provided by `languages=` query (not mirrored).
    languages: Vec<String>,
    /// Echo for response: multi-lang param, or legacy single language alone.
    languages_echo: Vec<String>,
    licenses: Vec<String>,
    topics: Vec<String>,
    topic_mode: TopicMode,
    q: Option<String>,
}

fn parse_filters(
    language: Option<&str>,
    languages_csv: Option<&str>,
    licenses_csv: Option<&str>,
    topics_csv: Option<&str>,
    topic_mode: Option<&str>,
    q: Option<&str>,
) -> Result<ParsedFilters, &'static str> {
    let topic_mode = parse_topic_mode(topic_mode).map_err(|_| "topic_mode must be and|or")?;
    let languages = parse_csv_list(languages_csv, false);
    let licenses = parse_csv_list(licenses_csv, false);
    let language = language
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let languages_echo = if !languages.is_empty() {
        languages.clone()
    } else if let Some(ref lang) = language {
        vec![lang.clone()]
    } else {
        vec![]
    };
    let topics = parse_csv_list(topics_csv, true);
    let q = q.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    Ok(ParsedFilters {
        language,
        languages,
        languages_echo,
        licenses,
        topics,
        topic_mode,
        q,
    })
}

pub fn languages_from_json(v: &serde_json::Value) -> Vec<LanguageShareDto> {
    match serde_json::from_value::<Vec<LanguageShare>>(v.clone()) {
        Ok(shares) => shares.into_iter().map(LanguageShareDto::from).collect(),
        Err(_) => vec![],
    }
}

fn to_items(
    rows: Vec<ght_core::models::LeaderboardRow>,
    tracked: &std::collections::HashSet<String>,
) -> Vec<LeaderboardItem> {
    rows.into_iter()
        .map(|r| {
            let mut languages = languages_from_json(&r.languages);
            // Until full language-share enrichment is populated for every repo,
            // fall back to the primary `language` so chips/filters/UI stay usable.
            if languages.is_empty() {
                if let Some(ref lang) = r.language {
                    if !lang.is_empty() {
                        languages.push(LanguageShareDto {
                            name: lang.clone(),
                            pct: 100.0,
                            bytes: None,
                        });
                    }
                }
            }
            let tracked_by_me = tracked.contains(&r.full_name);
            LeaderboardItem {
                rank: r.rank,
                full_name: r.full_name,
                html_url: r.html_url,
                description: r.description,
                language: r.language,
                license: r.license,
                topics: r.topics,
                languages,
                stars: r.stars,
                forks: r.forks,
                watchers: r.watchers,
                stars_today: r.stars_today,
                tracked_by_me,
            }
        })
        .collect()
}

/// v1 facets: unnest from the filtered result set (not disjunctive over board/date).
fn facets_from_items(
    items: &[LeaderboardItem],
) -> (Vec<TopicFacet>, Vec<LanguageFacet>, Vec<LicenseFacet>) {
    let mut topic_counts: HashMap<String, i64> = HashMap::new();
    let mut lang_counts: HashMap<String, i64> = HashMap::new();
    let mut license_counts: HashMap<String, i64> = HashMap::new();
    for item in items {
        for t in &item.topics {
            *topic_counts.entry(t.clone()).or_insert(0) += 1;
        }
        // Prefer multi-lang shares; fall back to primary language.
        if item.languages.is_empty() {
            if let Some(ref lang) = item.language {
                *lang_counts.entry(lang.clone()).or_insert(0) += 1;
            }
        } else {
            for share in &item.languages {
                *lang_counts.entry(share.name.clone()).or_insert(0) += 1;
            }
        }
        if let Some(ref lic) = item.license {
            if !lic.is_empty() {
                *license_counts.entry(lic.clone()).or_insert(0) += 1;
            }
        }
    }
    let mut topic_facets: Vec<TopicFacet> = topic_counts
        .into_iter()
        .map(|(topic, count)| TopicFacet { topic, count })
        .collect();
    topic_facets.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.topic.cmp(&b.topic)));
    let mut language_facets: Vec<LanguageFacet> = lang_counts
        .into_iter()
        .map(|(language, count)| LanguageFacet { language, count })
        .collect();
    language_facets.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.language.cmp(&b.language))
    });
    let mut license_facets: Vec<LicenseFacet> = license_counts
        .into_iter()
        .map(|(license, count)| LicenseFacet { license, count })
        .collect();
    license_facets.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.license.cmp(&b.license))
    });
    (topic_facets, language_facets, license_facets)
}

fn empty_resp(board: Board, parsed: &ParsedFilters) -> LeaderboardResp {
    LeaderboardResp {
        date: String::new(),
        board: board.as_str().to_string(),
        language: parsed.language.clone(),
        languages_filter: if parsed.languages_echo.is_empty() {
            None
        } else {
            Some(parsed.languages_echo.clone())
        },
        licenses_filter: if parsed.licenses.is_empty() {
            None
        } else {
            Some(parsed.licenses.clone())
        },
        q: parsed.q.clone(),
        topics_filter: if parsed.topics.is_empty() {
            None
        } else {
            Some(parsed.topics.clone())
        },
        topic_mode: Some(parsed.topic_mode.as_str().to_string()),
        items: vec![],
        topic_facets: Some(vec![]),
        language_facets: Some(vec![]),
        license_facets: Some(vec![]),
    }
}

fn ok_resp(
    board: Board,
    date: chrono::NaiveDate,
    parsed: &ParsedFilters,
    rows: Vec<ght_core::models::LeaderboardRow>,
    tracked: &std::collections::HashSet<String>,
) -> LeaderboardResp {
    let items = to_items(rows, tracked);
    let (topic_facets, language_facets, license_facets) = facets_from_items(&items);
    LeaderboardResp {
        date: date.format("%Y-%m-%d").to_string(),
        board: board.as_str().to_string(),
        language: parsed.language.clone(),
        languages_filter: if parsed.languages_echo.is_empty() {
            None
        } else {
            Some(parsed.languages_echo.clone())
        },
        licenses_filter: if parsed.licenses.is_empty() {
            None
        } else {
            Some(parsed.licenses.clone())
        },
        q: parsed.q.clone(),
        topics_filter: if parsed.topics.is_empty() {
            None
        } else {
            Some(parsed.topics.clone())
        },
        topic_mode: Some(parsed.topic_mode.as_str().to_string()),
        items,
        topic_facets: Some(topic_facets),
        language_facets: Some(language_facets),
        license_facets: Some(license_facets),
    }
}

/// Bind owned filter strings into a LeaderboardFilter with the right lifetimes.
fn store_filter<'a>(parsed: &'a ParsedFilters) -> LeaderboardFilter<'a> {
    LeaderboardFilter {
        language: parsed.language.as_deref(),
        languages: if parsed.languages.is_empty() {
            None
        } else {
            Some(parsed.languages.as_slice())
        },
        licenses: if parsed.licenses.is_empty() {
            None
        } else {
            Some(parsed.licenses.as_slice())
        },
        topics: if parsed.topics.is_empty() {
            None
        } else {
            Some(parsed.topics.as_slice())
        },
        topic_mode: parsed.topic_mode,
        q: parsed.q.as_deref(),
        // Public boards will pass true from API in a later task.
        exclude_archived: false,
        active_within_days: None,
    }
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
    RequireAuth(claims): RequireAuth,
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
    let parsed = match parse_filters(
        params.language.as_deref(),
        params.languages.as_deref(),
        params.licenses.as_deref(),
        params.topics.as_deref(),
        params.topic_mode.as_deref(),
        params.q.as_deref(),
    ) {
        Ok(p) => p,
        Err(msg) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": msg})),
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
        None => return Json(empty_resp(board, &parsed)).into_response(),
    };
    let filter = store_filter(&parsed);
    let rows = match board {
        Board::TopStars => store::top_by_stars(&state.pool, date, filter, 100).await,
        Board::TopForks => store::top_by_forks(&state.pool, date, filter, 100).await,
        Board::TopWatchers => store::top_by_watchers(&state.pool, date, filter, 100).await,
        _ => unreachable!(),
    };
    let tracked = crate::routes_track::tracked_full_names(&state.pool, claims.sub)
        .await
        .unwrap_or_default();
    match rows {
        Ok(rows) => {
            // Merge caller's tracked repos (same filters) so user-added repos
            // always participate in ranking/list even if outside top-N crawl.
            let rows = match merge_with_user_tracked(
                &state.pool,
                claims.sub,
                board,
                &parsed,
                rows,
            )
            .await
            {
                Ok(r) => r,
                Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            };
            Json(ok_resp(board, date, &parsed, rows, &tracked)).into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn trending(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
    Query(params): Query<TrendingParams>,
) -> impl IntoResponse {
    let board = Board::TrendingDaily;
    let parsed = match parse_filters(
        params.language.as_deref(),
        params.languages.as_deref(),
        params.licenses.as_deref(),
        params.topics.as_deref(),
        params.topic_mode.as_deref(),
        params.q.as_deref(),
    ) {
        Ok(p) => p,
        Err(msg) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": msg})),
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
        None => return Json(empty_resp(board, &parsed)).into_response(),
    };
    let filter = store_filter(&parsed);
    let tracked = crate::routes_track::tracked_full_names(&state.pool, claims.sub)
        .await
        .unwrap_or_default();
    match store::trending(&state.pool, date, filter, 100).await {
        Ok(rows) => {
            let rows = match merge_with_user_tracked(
                &state.pool,
                claims.sub,
                board,
                &parsed,
                rows,
            )
            .await
            {
                Ok(r) => r,
                Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            };
            Json(ok_resp(board, date, &parsed, rows, &tracked)).into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Pull the caller's tracked set through the same filters and merge into board
/// rows, re-ranking by the board metric.
async fn merge_with_user_tracked(
    pool: &sqlx::PgPool,
    user_id: i64,
    board: Board,
    parsed: &ParsedFilters,
    board_rows: Vec<ght_core::models::LeaderboardRow>,
) -> Result<Vec<ght_core::models::LeaderboardRow>, sqlx::Error> {
    let tracked_list = store::list_tracked(pool, user_id, store_filter(parsed)).await?;
    let tracked_rows: Vec<_> = tracked_list
        .into_iter()
        .map(store::tracked_row_to_leaderboard)
        .collect();
    let metric = |r: &ght_core::models::LeaderboardRow| -> i64 {
        match board {
            Board::TopForks => r.forks as i64,
            Board::TopWatchers => r.watchers.unwrap_or(0) as i64,
            Board::TrendingDaily => r.stars_today.unwrap_or(0) as i64,
            _ => r.stars as i64,
        }
    };
    Ok(store::merge_leaderboard_with_tracked(
        board_rows,
        tracked_rows,
        metric,
    ))
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

    use super::{parse_csv_list, parse_filters, parse_topic_mode, languages_from_json};
    use ght_core::models::TopicMode;

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

    fn repo(full_name: &str, lang: Option<&str>) -> RepoInput {
        let (owner, name) = full_name.split_once('/').unwrap();
        RepoInput {
            full_name: full_name.into(),
            owner: owner.into(),
            name: name.into(),
            html_url: format!("https://github.com/{full_name}"),
            language: lang.map(String::from),
            description: None,
            license: None,
            topics: vec![],
            languages_json: RepoInput::languages_empty(),
            language_names: vec![],
            pushed_at: None,
            archived: false,
            open_issues_count: None,
            created_at_gh: None,
            latest_release_at: None,
        }
    }

    fn repo_enriched(
        full_name: &str,
        lang: Option<&str>,
        topics: &[&str],
        languages: serde_json::Value,
        language_names: &[&str],
    ) -> RepoInput {
        let (owner, name) = full_name.split_once('/').unwrap();
        RepoInput {
            full_name: full_name.into(),
            owner: owner.into(),
            name: name.into(),
            html_url: format!("https://github.com/{full_name}"),
            language: lang.map(String::from),
            description: Some(format!("desc for {full_name}")),
            license: None,
            topics: topics.iter().map(|t| t.to_string()).collect(),
            languages_json: languages,
            language_names: language_names.iter().map(|s| s.to_string()).collect(),
            pushed_at: None,
            archived: false,
            open_issues_count: None,
            created_at_gh: None,
            latest_release_at: None,
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

    #[test]
    fn parse_csv_topics_lowercase_and_dedupe() {
        let v = parse_csv_list(Some(" AI, llm ,AI, "), true);
        assert_eq!(v, vec!["ai".to_string(), "llm".to_string()]);
    }

    #[test]
    fn parse_topic_mode_defaults_and() {
        assert_eq!(parse_topic_mode(None).unwrap(), TopicMode::And);
        assert_eq!(parse_topic_mode(Some("or")).unwrap(), TopicMode::Or);
        assert!(parse_topic_mode(Some("xor")).is_err());
    }

    #[test]
    fn legacy_language_echoes_but_not_multi_filter() {
        let p = parse_filters(Some("Rust"), None, None, None, None, None).unwrap();
        assert_eq!(p.language.as_deref(), Some("Rust"));
        assert!(p.languages.is_empty(), "legacy must not bind multi-lang SQL");
        assert_eq!(p.languages_echo, vec!["Rust".to_string()]);
    }

    #[test]
    fn languages_from_json_parses_shares() {
        let v = serde_json::json!([
            {"name": "Rust", "pct": 90.0, "bytes": 900},
            {"name": "Python", "pct": 10.0}
        ]);
        let shares = languages_from_json(&v);
        assert_eq!(shares.len(), 2);
        assert_eq!(shares[0].name, "Rust");
        assert_eq!(shares[0].pct, 90.0);
        assert_eq!(shares[0].bytes, Some(900));
        assert_eq!(shares[1].bytes, None);
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
        // Enriched fields always present.
        assert!(v["items"][0]["topics"].is_array());
        assert!(v["items"][0]["languages"].is_array());
        assert_eq!(v["items"][0]["tracked_by_me"], false);

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

    /// Task 4: trending + topics filter; items expose topics field.
    #[tokio::test]
    #[serial]
    async fn trending_topics_filter_and_item_fields() {
        let state = test_state().await;
        let date = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();
        let cookie = auth_cookie(&state);

        let shares_ai = serde_json::json!([
            {"name": "Python", "pct": 80.0, "bytes": 800},
            {"name": "Rust", "pct": 20.0, "bytes": 200}
        ]);
        let shares_other = serde_json::json!([{"name": "Go", "pct": 100.0, "bytes": 100}]);

        for (name, topics, langs_json, lang_names, stars_today) in [
            (
                "t/ai-bot",
                &["ai", "llm"][..],
                shares_ai.clone(),
                &["Python", "Rust"][..],
                50,
            ),
            (
                "t/web-app",
                &["web"][..],
                shares_other.clone(),
                &["Go"][..],
                40,
            ),
        ] {
            let id = store::upsert_repo(
                &state.pool,
                &repo_enriched(
                    name,
                    Some(lang_names[0]),
                    topics,
                    langs_json,
                    lang_names,
                ),
                date,
            )
            .await
            .unwrap();
            store::upsert_snapshot(
                &state.pool,
                id,
                date,
                Board::TrendingDaily,
                &SnapshotInput {
                    stars: stars_today * 10,
                    forks: 1,
                    watchers: None,
                    stars_today: Some(stars_today),
                },
            )
            .await
            .unwrap();
        }

        let (status, body) = get(
            state.clone(),
            "/api/leaderboard/trending?topics=ai",
            Some(cookie.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body={body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let items = v["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["full_name"], "t/ai-bot");
        assert!(items[0]["topics"].is_array());
        let topics: Vec<&str> = items[0]["topics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.as_str().unwrap())
            .collect();
        assert!(topics.contains(&"ai"));
        assert!(topics.contains(&"llm"));
        assert_eq!(items[0]["languages"].as_array().unwrap().len(), 2);
        assert_eq!(items[0]["languages"][0]["name"], "Python");
        assert_eq!(items[0]["tracked_by_me"], false);
        assert_eq!(v["topics_filter"][0], "ai");
        assert_eq!(v["topic_mode"], "and");

        // Facets from result set (only the matched repo).
        let tf = v["topic_facets"].as_array().unwrap();
        assert!(tf.iter().any(|f| f["topic"] == "ai" && f["count"] == 1));

        // Multi topics AND keeps only ai+llm repo; OR would include web if we asked web|ai.
        let (status, body) = get(
            state.clone(),
            "/api/leaderboard/trending?topics=ai,llm&topic_mode=and",
            Some(cookie.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 1);

        // languages multi OR via language_names.
        let (status, body) = get(
            state.clone(),
            "/api/leaderboard/trending?languages=Go",
            Some(cookie.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 1);
        assert_eq!(v["items"][0]["full_name"], "t/web-app");
        assert_eq!(v["languages_filter"][0], "Go");

        // q keyword on description/topic.
        let (status, body) = get(
            state.clone(),
            "/api/leaderboard/trending?q=ai-bot",
            Some(cookie),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 1);
        assert_eq!(v["items"][0]["full_name"], "t/ai-bot");
        assert_eq!(v["q"], "ai-bot");
    }

    #[tokio::test]
    #[serial]
    async fn invalid_topic_mode_rejected() {
        let state = test_state().await;
        seed(&state).await;
        let cookie = auth_cookie(&state);
        let (status, body) = get(
            state,
            "/api/leaderboard/trending?topic_mode=xor",
            Some(cookie),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("topic_mode"));
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
