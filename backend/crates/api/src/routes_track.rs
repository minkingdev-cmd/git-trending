//! User-tracked repos: lookup / track / untrack / list.

use crate::auth::extract::RequireAuth;
use crate::routes_leaderboard::{languages_from_json, LanguageShareDto};
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use ght_core::models::{Board, LanguageShare, LeaderboardFilter, RepoInput, SnapshotInput};
use ght_core::store::{self, TRACKED_REPO_LIMIT};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/repos/lookup", post(lookup))
        .route("/api/repos/track", post(track).delete(untrack))
        .route("/api/repos/tracked", get(list_tracked_api))
}

// ---------------------------------------------------------------------------
// parse_repo_ref
// ---------------------------------------------------------------------------

/// Parse `owner/name` or a github.com URL into (owner, name).
/// Rejects non-github hosts (SSRF guard).
pub fn parse_repo_ref(raw: &str) -> Result<(String, String), &'static str> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("empty repo ref");
    }

    let path = if let Some(rest) = strip_github_url(s) {
        rest
    } else if looks_like_url_or_host(s) {
        // Has a scheme or host-like prefix that is not github.com.
        return Err("only github.com URLs or owner/name allowed");
    } else {
        s
    };

    let path = path.split(['?', '#']).next().unwrap_or(path);
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);

    let mut parts = path.split('/');
    let owner = parts.next().unwrap_or("").trim();
    let name = parts.next().unwrap_or("").trim();
    if owner.is_empty() || name.is_empty() {
        return Err("expected owner/name");
    }
    if parts.next().is_some() {
        return Err("too many path segments");
    }
    if !is_valid_github_segment(owner) || !is_valid_github_segment(name) {
        return Err("invalid owner or name");
    }
    Ok((owner.to_string(), name.to_string()))
}

fn strip_github_url(s: &str) -> Option<&str> {
    const PREFIXES: &[&str] = &[
        "https://github.com/",
        "http://github.com/",
        "https://www.github.com/",
        "http://www.github.com/",
        "github.com/",
        "www.github.com/",
    ];
    for p in PREFIXES {
        if let Some(rest) = s.strip_prefix(p) {
            return Some(rest);
        }
    }
    // Case-insensitive host match for https://GitHub.com/...
    let lower = s.to_ascii_lowercase();
    for p in &[
        "https://github.com/",
        "http://github.com/",
        "https://www.github.com/",
        "http://www.github.com/",
    ] {
        if lower.starts_with(p) {
            return Some(&s[p.len()..]);
        }
    }
    None
}

fn looks_like_url_or_host(s: &str) -> bool {
    s.contains("://")
        || s.starts_with("www.")
        || (s.contains('.')
            && s.split('/')
                .next()
                .map(|h| h.contains('.'))
                .unwrap_or(false))
}

fn is_valid_github_segment(s: &str) -> bool {
    // GitHub owner/name: alphanumerics, hyphen, underscore, dot; no leading/trailing junk.
    !s.is_empty()
        && s.len() <= 100
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct RepoRefBody {
    pub full_name: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UntrackParams {
    full_name: String,
}

#[derive(Debug, Deserialize)]
struct TrackedListParams {
    pub q: Option<String>,
    pub topics: Option<String>,
    pub languages: Option<String>,
    pub topic_mode: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TrackedRepoItem {
    pub full_name: String,
    pub html_url: String,
    pub description: Option<String>,
    pub languages: Vec<LanguageShareDto>,
    pub topics: Vec<String>,
    pub stars: i32,
    pub forks: i32,
    pub watchers: Option<i32>,
    pub status: String,
    pub added_at: String,
}

#[derive(Debug, Serialize)]
struct LookupResp {
    full_name: String,
    html_url: String,
    description: Option<String>,
    languages: Vec<LanguageShareDto>,
    topics: Vec<String>,
    stars: i32,
    forks: i32,
    watchers: Option<i32>,
    on_leaderboard: bool,
    already_tracked: bool,
}

// ---------------------------------------------------------------------------
// GitHub REST helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct GhRepo {
    full_name: String,
    owner: String,
    name: String,
    html_url: String,
    description: Option<String>,
    language: Option<String>,
    topics: Vec<String>,
    stars: i32,
    forks: i32,
    watchers: Option<i32>,
    languages: Vec<LanguageShare>,
    language_names: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct GhRepoJson {
    full_name: String,
    html_url: String,
    description: Option<String>,
    language: Option<String>,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default)]
    private: bool,
    stargazers_count: i32,
    forks_count: i32,
    #[serde(default)]
    subscribers_count: Option<i32>,
    owner: Option<GhOwner>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhOwner {
    login: String,
}

enum GhError {
    NotFound,
    Other(String),
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("gh-trending-api/0.1")
        .build()
        .expect("reqwest client")
}

fn normalize_topics(raw: &[String]) -> Vec<String> {
    let mut out: Vec<String> = raw
        .iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

fn shares_from_language_map(map: HashMap<String, i64>) -> (Vec<LanguageShare>, Vec<String>) {
    let total: i64 = map.values().copied().sum();
    let mut pairs: Vec<(String, i64)> = map.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let shares: Vec<LanguageShare> = pairs
        .into_iter()
        .map(|(name, bytes)| {
            let pct = if total > 0 {
                (bytes as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            LanguageShare {
                name,
                pct,
                bytes: Some(bytes),
            }
        })
        .collect();
    let names: Vec<String> = shares.iter().map(|s| s.name.clone()).collect();
    (shares, names)
}

async fn fetch_github_repo(
    base: &str,
    token: Option<&str>,
    owner: &str,
    name: &str,
) -> Result<GhRepo, GhError> {
    let client = http_client();
    let url = format!("{base}/repos/{owner}/{name}");
    let mut req = client
        .get(&url)
        .header("Accept", "application/vnd.github+json");
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| GhError::Other(e.to_string()))?;
    let status = resp.status();
    if status.as_u16() == 404 || status.as_u16() == 403 {
        return Err(GhError::NotFound);
    }
    if !status.is_success() {
        return Err(GhError::Other(format!("github status {status}")));
    }
    let meta: GhRepoJson = resp
        .json()
        .await
        .map_err(|e| GhError::Other(e.to_string()))?;
    if meta.private {
        return Err(GhError::NotFound);
    }

    // Languages (best-effort; empty on failure).
    let (languages, language_names) =
        match fetch_languages(&client, base, token, owner, name).await {
            Ok(v) => v,
            Err(_) => (vec![], vec![]),
        };

    let owner_login = meta
        .owner
        .map(|o| o.login)
        .unwrap_or_else(|| owner.to_string());
    let repo_name = meta.name.unwrap_or_else(|| name.to_string());
    let full_name = if meta.full_name.contains('/') {
        meta.full_name
    } else {
        format!("{owner_login}/{repo_name}")
    };

    Ok(GhRepo {
        full_name,
        owner: owner_login,
        name: repo_name,
        html_url: meta.html_url,
        description: meta.description,
        language: meta.language,
        topics: normalize_topics(&meta.topics),
        stars: meta.stargazers_count,
        forks: meta.forks_count,
        watchers: meta.subscribers_count,
        languages,
        language_names,
    })
}

async fn fetch_languages(
    client: &reqwest::Client,
    base: &str,
    token: Option<&str>,
    owner: &str,
    name: &str,
) -> Result<(Vec<LanguageShare>, Vec<String>), GhError> {
    let url = format!("{base}/repos/{owner}/{name}/languages");
    let mut req = client
        .get(&url)
        .header("Accept", "application/vnd.github+json");
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| GhError::Other(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(GhError::Other(format!(
            "languages status {}",
            resp.status()
        )));
    }
    let map: HashMap<String, i64> = resp
        .json()
        .await
        .map_err(|e| GhError::Other(e.to_string()))?;
    Ok(shares_from_language_map(map))
}

// ---------------------------------------------------------------------------
// Status / DB helpers
// ---------------------------------------------------------------------------

fn resolve_ref_body(body: &RepoRefBody) -> Result<(String, String), &'static str> {
    if let Some(ref fn_) = body.full_name {
        let t = fn_.trim();
        if !t.is_empty() {
            return parse_repo_ref(t);
        }
    }
    if let Some(ref url) = body.url {
        let t = url.trim();
        if !t.is_empty() {
            return parse_repo_ref(t);
        }
    }
    Err("full_name or url required")
}

/// `on_board` if repo has a snapshot on any public board for `date`.
async fn is_on_public_board(
    pool: &sqlx::PgPool,
    full_name: &str,
    date: chrono::NaiveDate,
) -> Result<bool, sqlx::Error> {
    let rec: (bool,) = sqlx::query_as(
        r#"SELECT EXISTS(
               SELECT 1
               FROM snapshots s
               JOIN repos r ON r.id = s.repo_id
               WHERE r.full_name = $1
                 AND s.snapshot_date = $2
                 AND s.board IN ('top_stars', 'top_forks', 'top_watchers', 'trending_daily')
           )"#,
    )
    .bind(full_name)
    .bind(date)
    .fetch_one(pool)
    .await?;
    Ok(rec.0)
}

async fn has_any_snapshot(pool: &sqlx::PgPool, full_name: &str) -> Result<bool, sqlx::Error> {
    let rec: (bool,) = sqlx::query_as(
        r#"SELECT EXISTS(
               SELECT 1
               FROM snapshots s
               JOIN repos r ON r.id = s.repo_id
               WHERE r.full_name = $1
           )"#,
    )
    .bind(full_name)
    .fetch_one(pool)
    .await?;
    Ok(rec.0)
}

async fn compute_status(
    pool: &sqlx::PgPool,
    full_name: &str,
    today: chrono::NaiveDate,
) -> Result<&'static str, sqlx::Error> {
    if is_on_public_board(pool, full_name, today).await? {
        return Ok("on_board");
    }
    if has_any_snapshot(pool, full_name).await? {
        return Ok("tracking");
    }
    Ok("pending")
}

/// Full names tracked by this user (max 50).
pub async fn tracked_full_names(
    pool: &sqlx::PgPool,
    user_id: i64,
) -> Result<std::collections::HashSet<String>, sqlx::Error> {
    let rows: Vec<(String,)> = sqlx::query_as(
        r#"SELECT r.full_name
           FROM user_tracked_repos t
           JOIN repos r ON r.id = t.repo_id
           WHERE t.user_id = $1"#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

async fn is_tracked_full_name(
    pool: &sqlx::PgPool,
    user_id: i64,
    full_name: &str,
) -> Result<bool, sqlx::Error> {
    let rec: (bool,) = sqlx::query_as(
        r#"SELECT EXISTS(
               SELECT 1
               FROM user_tracked_repos t
               JOIN repos r ON r.id = t.repo_id
               WHERE t.user_id = $1 AND r.full_name = $2
           )"#,
    )
    .bind(user_id)
    .bind(full_name)
    .fetch_one(pool)
    .await?;
    Ok(rec.0)
}

fn parse_csv(raw: Option<&str>, lowercase: bool) -> Vec<String> {
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
    let mut seen = std::collections::HashSet::new();
    out.retain(|x| seen.insert(x.clone()));
    out
}

fn gh_to_item(
    gh: &GhRepo,
    status: &str,
    added_at: chrono::DateTime<chrono::Utc>,
) -> TrackedRepoItem {
    TrackedRepoItem {
        full_name: gh.full_name.clone(),
        html_url: gh.html_url.clone(),
        description: gh.description.clone(),
        languages: gh
            .languages
            .iter()
            .cloned()
            .map(LanguageShareDto::from)
            .collect(),
        topics: gh.topics.clone(),
        stars: gh.stars,
        forks: gh.forks,
        watchers: gh.watchers,
        status: status.to_string(),
        added_at: added_at.to_rfc3339(),
    }
}

fn tracked_row_to_item(
    row: ght_core::models::TrackedRow,
    status: &str,
) -> TrackedRepoItem {
    TrackedRepoItem {
        full_name: row.full_name,
        html_url: row.html_url,
        description: row.description,
        languages: languages_from_json(&row.languages),
        topics: row.topics,
        stars: row.stars.unwrap_or(0),
        forks: row.forks.unwrap_or(0),
        watchers: row.watchers,
        status: status.to_string(),
        added_at: row.created_at.to_rfc3339(),
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn lookup(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
    Json(body): Json<RepoRefBody>,
) -> impl IntoResponse {
    let (owner, name) = match resolve_ref_body(&body) {
        Ok(v) => v,
        Err(msg) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": msg})),
            )
                .into_response();
        }
    };

    let gh = match fetch_github_repo(
        &state.settings.github_api_base,
        state.settings.github_token.as_deref(),
        &owner,
        &name,
    )
    .await
    {
        Ok(g) => g,
        Err(GhError::NotFound) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "repo not found or private"})),
            )
                .into_response();
        }
        Err(GhError::Other(e)) => {
            tracing::warn!(error = %e, "github lookup failed");
            return StatusCode::BAD_GATEWAY.into_response();
        }
    };

    let today = Utc::now().date_naive();
    let on_leaderboard = match is_on_public_board(&state.pool, &gh.full_name, today).await {
        Ok(v) => v,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let already_tracked =
        match is_tracked_full_name(&state.pool, claims.sub, &gh.full_name).await {
            Ok(v) => v,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };

    Json(LookupResp {
        full_name: gh.full_name,
        html_url: gh.html_url,
        description: gh.description,
        languages: gh
            .languages
            .into_iter()
            .map(LanguageShareDto::from)
            .collect(),
        topics: gh.topics,
        stars: gh.stars,
        forks: gh.forks,
        watchers: gh.watchers,
        on_leaderboard,
        already_tracked,
    })
    .into_response()
}

async fn track(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
    Json(body): Json<RepoRefBody>,
) -> impl IntoResponse {
    let (owner, name) = match resolve_ref_body(&body) {
        Ok(v) => v,
        Err(msg) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": msg})),
            )
                .into_response();
        }
    };

    let gh = match fetch_github_repo(
        &state.settings.github_api_base,
        state.settings.github_token.as_deref(),
        &owner,
        &name,
    )
    .await
    {
        Ok(g) => g,
        Err(GhError::NotFound) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "repo not found or private"})),
            )
                .into_response();
        }
        Err(GhError::Other(e)) => {
            tracing::warn!(error = %e, "github track fetch failed");
            return StatusCode::BAD_GATEWAY.into_response();
        }
    };

    // Gate limit / side effects BEFORE any writes so 409 leaves DB unchanged.
    // (Re-track of an already-tracked repo may still refresh meta/snapshot below.)
    let already =
        match is_tracked_full_name(&state.pool, claims.sub, &gh.full_name).await {
            Ok(v) => v,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };

    if !already {
        let count = match store::count_tracked(&state.pool, claims.sub).await {
            Ok(c) => c,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
        if count >= TRACKED_REPO_LIMIT {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "tracked repo limit reached",
                    "limit": TRACKED_REPO_LIMIT
                })),
            )
                .into_response();
        }
    }

    let today = Utc::now().date_naive();
    let languages_json = serde_json::to_value(&gh.languages).unwrap_or_else(|_| serde_json::json!([]));
    let repo_input = RepoInput {
        full_name: gh.full_name.clone(),
        owner: gh.owner.clone(),
        name: gh.name.clone(),
        html_url: gh.html_url.clone(),
        language: gh.language.clone().or_else(|| {
            gh.language_names.first().cloned()
        }),
        description: gh.description.clone(),
        topics: gh.topics.clone(),
        languages_json,
        language_names: gh.language_names.clone(),
    };

    let repo_id = match store::upsert_repo(&state.pool, &repo_input, today).await {
        Ok(id) => id,
        Err(e) => {
            tracing::error!(error = %e, "upsert_repo on track");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Instant snapshot so status leaves pending quickly.
    if let Err(e) = store::upsert_snapshot(
        &state.pool,
        repo_id,
        today,
        Board::TrackedDaily,
        &SnapshotInput {
            stars: gh.stars,
            forks: gh.forks,
            watchers: gh.watchers,
            stars_today: None,
        },
    )
    .await
    {
        tracing::warn!(error = %e, "tracked_daily snapshot on track failed");
        // non-fatal: tracking still proceeds
    }

    if !already {
        if let Err(e) = store::track_repo(&state.pool, claims.sub, repo_id).await {
            tracing::error!(error = %e, "track_repo");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    let status = match compute_status(&state.pool, &gh.full_name, today).await {
        Ok(s) => s,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    // Prefer DB created_at when already tracked.
    let added_at = if already {
        match store::list_tracked(&state.pool, claims.sub, LeaderboardFilter::empty()).await {
            Ok(rows) => rows
                .into_iter()
                .find(|r| r.full_name == gh.full_name)
                .map(|r| r.created_at)
                .unwrap_or_else(Utc::now),
            Err(_) => Utc::now(),
        }
    } else {
        Utc::now()
    };

    let item = gh_to_item(&gh, status, added_at);
    let code = if already {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    (code, Json(serde_json::json!({ "item": item }))).into_response()
}

async fn untrack(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
    Query(params): Query<UntrackParams>,
) -> impl IntoResponse {
    let full_name = params.full_name.trim();
    if full_name.is_empty() || !full_name.contains('/') {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "full_name required as owner/name"})),
        )
            .into_response();
    }
    // Validate shape (reject evil hosts if a URL was passed).
    if parse_repo_ref(full_name).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid full_name"})),
        )
            .into_response();
    }
    match store::untrack_repo(&state.pool, claims.sub, full_name).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn list_tracked_api(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
    Query(params): Query<TrackedListParams>,
) -> impl IntoResponse {
    let topics = parse_csv(params.topics.as_deref(), true);
    let languages = parse_csv(params.languages.as_deref(), false);
    let language = params
        .language
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let q = params
        .q
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let topic_mode = match params.topic_mode.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => ght_core::models::TopicMode::And,
        Some(s) => match ght_core::models::TopicMode::parse(&s.to_lowercase()) {
            Some(m) => m,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "topic_mode must be and|or"})),
                )
                    .into_response();
            }
        },
    };

    let filter = LeaderboardFilter {
        language: language.as_deref(),
        languages: if languages.is_empty() {
            None
        } else {
            Some(languages.as_slice())
        },
        topics: if topics.is_empty() {
            None
        } else {
            Some(topics.as_slice())
        },
        topic_mode,
        q: q.as_deref(),
    };

    let rows = match store::list_tracked(&state.pool, claims.sub, filter).await {
        Ok(r) => r,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let today = Utc::now().date_naive();
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let status = match compute_status(&state.pool, &row.full_name, today).await {
            Ok(s) => s,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
        items.push(tracked_row_to_item(row, status));
    }

    Json(serde_json::json!({ "items": items })).into_response()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::tokens;
    use crate::state::AppState;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use ght_core::models::{Board, LeaderboardFilter, RepoInput, SnapshotInput};
    use ght_core::{db, store};
    use serial_test::serial;
    use tower::ServiceExt;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn parse_repo_ref_url_and_slug() {
        assert_eq!(
            parse_repo_ref("https://github.com/a/b").unwrap(),
            ("a".into(), "b".into())
        );
        assert_eq!(
            parse_repo_ref("https://github.com/a/b.git").unwrap(),
            ("a".into(), "b".into())
        );
        assert_eq!(
            parse_repo_ref("http://www.github.com/Foo/Bar").unwrap(),
            ("Foo".into(), "Bar".into())
        );
        assert_eq!(
            parse_repo_ref("github.com/x/y").unwrap(),
            ("x".into(), "y".into())
        );
        assert_eq!(
            parse_repo_ref("owner/name").unwrap(),
            ("owner".into(), "name".into())
        );
        assert_eq!(
            parse_repo_ref("  owner/name  ").unwrap(),
            ("owner".into(), "name".into())
        );
    }

    #[test]
    fn parse_repo_ref_rejects_evil_hosts() {
        assert!(parse_repo_ref("https://evil.com/a/b").is_err());
        assert!(parse_repo_ref("http://evil.com/a/b").is_err());
        assert!(parse_repo_ref("https://github.com.evil.com/a/b").is_err());
        assert!(parse_repo_ref("https://notgithub.com/a/b").is_err());
        assert!(parse_repo_ref("").is_err());
        assert!(parse_repo_ref("onlyowner").is_err());
        assert!(parse_repo_ref("a/b/c").is_err());
    }

    async fn test_state(github_base: Option<&str>) -> AppState {
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
        let base = github_base.map(|s| s.to_string());
        let settings = ght_core::config::Settings::from_map(|k| match k {
            "DATABASE_URL" => Some(url.clone()),
            "JWT_SECRET" => Some("test-secret".into()),
            "GITHUB_API_BASE" => base.clone(),
            "GITHUB_TOKEN" => Some("test-tok".into()),
            _ => None,
        })
        .unwrap();
        AppState { pool, settings }
    }

    fn auth_cookie(state: &AppState, user_id: i64) -> String {
        let jwt = tokens::issue_access(&state.settings.jwt_secret, user_id, "tracker").unwrap();
        format!("access_token={jwt}")
    }

    async fn call(
        state: AppState,
        method: &str,
        uri: &str,
        cookie: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, String) {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(c) = cookie {
            builder = builder.header("cookie", c);
        }
        let req_body = if let Some(b) = body {
            builder = builder.header("content-type", "application/json");
            Body::from(serde_json::to_vec(&b).unwrap())
        } else {
            Body::empty()
        };
        let resp = crate::build_router(state)
            .oneshot(builder.body(req_body).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    fn gh_repo_body(owner: &str, name: &str, private: bool) -> String {
        format!(
            r#"{{
                "full_name": "{owner}/{name}",
                "html_url": "https://github.com/{owner}/{name}",
                "description": "A cool repo",
                "language": "Rust",
                "topics": ["ai", "LLM"],
                "private": {private},
                "stargazers_count": 42,
                "forks_count": 7,
                "subscribers_count": 3,
                "owner": {{"login": "{owner}"}},
                "name": "{name}"
            }}"#,
            private = if private { "true" } else { "false" }
        )
    }

    async fn mount_repo(server: &MockServer, owner: &str, name: &str, private: bool) {
        Mock::given(method("GET"))
            .and(path(format!("/repos/{owner}/{name}")))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(gh_repo_body(owner, name, private)),
            )
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/repos/{owner}/{name}/languages")))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(r#"{"Rust":900,"Python":100}"#),
            )
            .mount(server)
            .await;
    }

    #[tokio::test]
    #[serial]
    async fn track_is_idempotent_and_list_untrack() {
        let server = MockServer::start().await;
        mount_repo(&server, "acme", "widget", false).await;

        let state = test_state(Some(&server.uri())).await;
        let cookie = auth_cookie(&state, 1);

        // First track → 201
        let (status, body) = call(
            state.clone(),
            "POST",
            "/api/repos/track",
            Some(&cookie),
            Some(serde_json::json!({"full_name": "acme/widget"})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "body={body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["item"]["full_name"], "acme/widget");
        assert_eq!(v["item"]["stars"], 42);
        assert!(
            v["item"]["status"] == "tracking" || v["item"]["status"] == "on_board",
            "status={}",
            v["item"]["status"]
        );
        assert!(v["item"]["topics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t == "ai"));
        assert_eq!(v["item"]["languages"].as_array().unwrap().len(), 2);

        // Second track → 200 (idempotent)
        let (status, body) = call(
            state.clone(),
            "POST",
            "/api/repos/track",
            Some(&cookie),
            Some(serde_json::json!({"url": "https://github.com/acme/widget"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body={body}");
        assert_eq!(
            store::count_tracked(&state.pool, 1).await.unwrap(),
            1,
            "must not double-insert"
        );

        // List
        let (status, body) =
            call(state.clone(), "GET", "/api/repos/tracked", Some(&cookie), None).await;
        assert_eq!(status, StatusCode::OK, "body={body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 1);
        assert_eq!(v["items"][0]["full_name"], "acme/widget");

        // Lookup shows already_tracked
        let (status, body) = call(
            state.clone(),
            "POST",
            "/api/repos/lookup",
            Some(&cookie),
            Some(serde_json::json!({"full_name": "acme/widget"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body={body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["already_tracked"], true);
        assert_eq!(v["full_name"], "acme/widget");

        // Untrack
        let (status, _) = call(
            state.clone(),
            "DELETE",
            "/api/repos/track?full_name=acme/widget",
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(store::count_tracked(&state.pool, 1).await.unwrap(), 0);

        let (status, body) =
            call(state.clone(), "GET", "/api/repos/tracked", Some(&cookie), None).await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v["items"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    #[serial]
    async fn track_rejects_private_and_evil_host_and_limit() {
        let server = MockServer::start().await;
        mount_repo(&server, "sec", "private-thing", true).await;
        Mock::given(method("GET"))
            .and(path("/repos/missing/nope"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let state = test_state(Some(&server.uri())).await;
        let cookie = auth_cookie(&state, 9);

        // private → 404
        let (status, _) = call(
            state.clone(),
            "POST",
            "/api/repos/track",
            Some(&cookie),
            Some(serde_json::json!({"full_name": "sec/private-thing"})),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // 404
        let (status, _) = call(
            state.clone(),
            "POST",
            "/api/repos/lookup",
            Some(&cookie),
            Some(serde_json::json!({"full_name": "missing/nope"})),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // evil host → 400
        let (status, body) = call(
            state.clone(),
            "POST",
            "/api/repos/track",
            Some(&cookie),
            Some(serde_json::json!({"url": "https://evil.com/a/b"})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body={body}");

        // limit 409: seed TRACKED_REPO_LIMIT tracks, then try one more
        let date = Utc::now().date_naive();
        for i in 0..TRACKED_REPO_LIMIT {
            let name = format!("limitseed/r{i}");
            let id = store::upsert_repo(
                &state.pool,
                &RepoInput {
                    full_name: name.clone(),
                    owner: "limitseed".into(),
                    name: format!("r{i}"),
                    html_url: format!("https://github.com/{name}"),
                    language: None,
                    description: None,
                    topics: vec![],
                    languages_json: RepoInput::languages_empty(),
                    language_names: vec![],
                },
                date,
            )
            .await
            .unwrap();
            store::track_repo(&state.pool, 9, id).await.unwrap();
        }
        // Mount a 51st public repo
        mount_repo(&server, "overflow", "one", false).await;
        let (status, body) = call(
            state.clone(),
            "POST",
            "/api/repos/track",
            Some(&cookie),
            Some(serde_json::json!({"full_name": "overflow/one"})),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "body={body}");

        // 409 must not mutate: no extra track row, overflow not listed, no snapshot.
        assert_eq!(
            store::count_tracked(&state.pool, 9).await.unwrap(),
            TRACKED_REPO_LIMIT as i64
        );
        let listed = store::list_tracked(&state.pool, 9, LeaderboardFilter::empty())
            .await
            .unwrap();
        assert!(
            listed.iter().all(|r| r.full_name != "overflow/one"),
            "overflow/one must not appear in tracked list after 409"
        );
        let snap_cnt: (i64,) = sqlx::query_as(
            r#"SELECT COUNT(*)::bigint
               FROM snapshots s
               JOIN repos r ON r.id = s.repo_id
               WHERE r.full_name = $1 AND s.board = 'tracked_daily'"#,
        )
        .bind("overflow/one")
        .fetch_one(&state.pool)
        .await
        .unwrap();
        assert_eq!(
            snap_cnt.0, 0,
            "409 must not write tracked_daily snapshot for overflow repo"
        );
        let repo_cnt: (i64,) = sqlx::query_as(
            r#"SELECT COUNT(*)::bigint FROM repos WHERE full_name = $1"#,
        )
        .bind("overflow/one")
        .fetch_one(&state.pool)
        .await
        .unwrap();
        assert_eq!(
            repo_cnt.0, 0,
            "409 must not upsert repos row for overflow repo"
        );
    }

    #[tokio::test]
    #[serial]
    async fn track_requires_auth() {
        let state = test_state(None).await;
        let (status, _) = call(
            state,
            "POST",
            "/api/repos/track",
            None,
            Some(serde_json::json!({"full_name": "a/b"})),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    #[serial]
    async fn history_allows_own_tracked_daily() {
        let server = MockServer::start().await;
        mount_repo(&server, "hist", "tracked-only", false).await;

        let state = test_state(Some(&server.uri())).await;
        let cookie = auth_cookie(&state, 3);

        // Track via API (writes tracked_daily snapshot).
        let (status, body) = call(
            state.clone(),
            "POST",
            "/api/repos/track",
            Some(&cookie),
            Some(serde_json::json!({"full_name": "hist/tracked-only"})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "body={body}");

        // History on tracked_daily for own tracked repo.
        let (status, body) = call(
            state.clone(),
            "GET",
            "/api/repo/history?full_name=hist/tracked-only&board=tracked_daily&days=7",
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body={body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["board"], "tracked_daily");
        let points = v["points"].as_array().unwrap();
        assert!(!points.is_empty(), "expected snapshot points");
        assert_eq!(points[0]["stars"], 42);

        // Other user cannot read tracked_daily history for this repo.
        let other = auth_cookie(&state, 99);
        let (status, _) = call(
            state.clone(),
            "GET",
            "/api/repo/history?full_name=hist/tracked-only&board=tracked_daily&days=7",
            Some(&other),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // Leaderboard tracked_by_me
        // Seed a public-board row for same day so top endpoint has data.
        let today = Utc::now().date_naive();
        let rid: i64 = sqlx::query_scalar("SELECT id FROM repos WHERE full_name = $1")
            .bind("hist/tracked-only")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        store::upsert_snapshot(
            &state.pool,
            rid,
            today,
            Board::TopStars,
            &SnapshotInput {
                stars: 42,
                forks: 7,
                watchers: Some(3),
                stars_today: None,
            },
        )
        .await
        .unwrap();

        let (status, body) = call(
            state.clone(),
            "GET",
            &format!(
                "/api/leaderboard/top?metric=stars&date={}",
                today.format("%Y-%m-%d")
            ),
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body={body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let item = v["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["full_name"] == "hist/tracked-only")
            .expect("repo on board");
        assert_eq!(item["tracked_by_me"], true);

        // After untrack, flag is false.
        let (status, _) = call(
            state.clone(),
            "DELETE",
            "/api/repos/track?full_name=hist/tracked-only",
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, body) = call(
            state.clone(),
            "GET",
            &format!(
                "/api/leaderboard/top?metric=stars&date={}",
                today.format("%Y-%m-%d")
            ),
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let item = v["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["full_name"] == "hist/tracked-only")
            .unwrap();
        assert_eq!(item["tracked_by_me"], false);
    }

    #[tokio::test]
    #[serial]
    async fn status_on_board_when_public_snapshot_today() {
        let server = MockServer::start().await;
        mount_repo(&server, "pub", "boarded", false).await;
        let state = test_state(Some(&server.uri())).await;
        let cookie = auth_cookie(&state, 5);
        let today = Utc::now().date_naive();

        // Put repo on public board first.
        let id = store::upsert_repo(
            &state.pool,
            &RepoInput {
                full_name: "pub/boarded".into(),
                owner: "pub".into(),
                name: "boarded".into(),
                html_url: "https://github.com/pub/boarded".into(),
                language: Some("Rust".into()),
                description: Some("x".into()),
                topics: vec!["ai".into()],
                languages_json: serde_json::json!([{"name":"Rust","pct":100.0}]),
                language_names: vec!["Rust".into()],
            },
            today,
        )
        .await
        .unwrap();
        store::upsert_snapshot(
            &state.pool,
            id,
            today,
            Board::TrendingDaily,
            &SnapshotInput {
                stars: 10,
                forks: 1,
                watchers: None,
                stars_today: Some(5),
            },
        )
        .await
        .unwrap();

        let (status, body) = call(
            state.clone(),
            "POST",
            "/api/repos/track",
            Some(&cookie),
            Some(serde_json::json!({"full_name": "pub/boarded"})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "body={body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["item"]["status"], "on_board");
    }
}
