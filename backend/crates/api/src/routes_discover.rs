//! Discover: proxy GitHub Search for out-of-index repos.
//!
//! - `GET /api/discover/search` — RequireAuth; hybrid token (user PAT → GITHUB_TOKEN)
//! - Never writes `repos` / `snapshots` (track path does that)
//! - Rate-limit **before** calling GitHub

use crate::auth::extract::RequireAuth;
use crate::rate_limit::{RateLimitError, RateLimitScope};
use crate::routes_leaderboard::{parse_active_within, parse_exclude_archived};
use crate::routes_track::tracked_full_names;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use ght_core::crypto;
use ght_core::health::compute_health;
use ght_core::license::license_from_gh;
use ght_core::store;
use ght_core::users;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const PER_PAGE: u32 = 30;
const MAX_PAGE: u32 = 10;
const MAX_USER_Q_LEN: usize = 256;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/discover/search", get(search))
}

// ---------------------------------------------------------------------------
// Query builder
// ---------------------------------------------------------------------------

/// Compile discover filters into a GitHub Search `q` string.
///
/// Pieces (space-joined, skip empty): free-text `user_q`, `language:…`,
/// `license:…`, `stars:>=N`, `archived:false`, `pushed:>YYYY-MM-DD`.
pub fn build_discover_q(
    user_q: &str,
    language: Option<&str>,
    license: Option<&str>,
    min_stars: Option<u32>,
    exclude_archived: bool,
    active_within_days: Option<u32>,
    now: DateTime<Utc>,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    let q = user_q.trim();
    if !q.is_empty() {
        parts.push(q.to_string());
    }

    if let Some(lang) = language.map(str::trim).filter(|s| !s.is_empty()) {
        parts.push(format!("language:{}", quote_qualifier_value(lang)));
    }

    if let Some(lic) = license.map(str::trim).filter(|s| !s.is_empty()) {
        parts.push(format!("license:{}", quote_qualifier_value(lic)));
    }

    if let Some(n) = min_stars {
        parts.push(format!("stars:>={n}"));
    }

    if exclude_archived {
        parts.push("archived:false".to_string());
    }

    if let Some(days) = active_within_days {
        let cutoff = now - Duration::days(days as i64);
        parts.push(format!("pushed:>{}", cutoff.format("%Y-%m-%d")));
    }

    parts.join(" ")
}

/// Quote values that need it for GitHub Search (spaces / special chars).
fn quote_qualifier_value(v: &str) -> String {
    if v.chars().any(|c| c.is_whitespace() || c == '"' || c == ':') {
        let escaped = v.replace('"', r#"\""#);
        format!("\"{escaped}\"")
    } else {
        v.to_string()
    }
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DiscoverSearchParams {
    pub q: Option<String>,
    pub language: Option<String>,
    pub license: Option<String>,
    pub min_stars: Option<String>,
    /// Default `1` (exclude archived).
    pub exclude_archived: Option<String>,
    pub active_within: Option<String>,
    /// `stars` | `updated`; default `stars`.
    pub sort: Option<String>,
    /// 1..=10; default 1.
    pub page: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    User,
    Shared,
}

impl AuthMode {
    fn as_str(self) -> &'static str {
        match self {
            AuthMode::User => "user",
            AuthMode::Shared => "shared",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct DiscoverItem {
    pub full_name: String,
    pub html_url: String,
    pub description: Option<String>,
    pub language: Option<String>,
    pub license: Option<String>,
    pub stars: i32,
    pub forks: i32,
    pub topics: Vec<String>,
    pub pushed_at: Option<DateTime<Utc>>,
    pub archived: bool,
    pub open_issues_count: Option<i32>,
    pub created_at_gh: Option<DateTime<Utc>>,
    /// Always null on discover list (no release secondary request).
    pub latest_release_at: Option<DateTime<Utc>>,
    pub health: String,
    pub already_tracked: bool,
    pub in_local_index: bool,
}

#[derive(Debug, Serialize)]
pub struct DiscoverSearchResp {
    pub items: Vec<DiscoverItem>,
    pub page: u32,
    pub per_page: u32,
    pub total_count: i64,
    pub incomplete_results: bool,
    pub auth_mode: AuthMode,
}

// ---------------------------------------------------------------------------
// GitHub Search JSON
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct GhSearchResponse {
    total_count: i64,
    #[serde(default)]
    incomplete_results: bool,
    #[serde(default)]
    items: Vec<GhSearchItem>,
}

#[derive(Debug, Deserialize)]
struct GhSearchItem {
    full_name: String,
    html_url: String,
    description: Option<String>,
    language: Option<String>,
    license: Option<GhLicenseJson>,
    stargazers_count: i32,
    forks_count: i32,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default)]
    pushed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    open_issues_count: Option<i32>,
    #[serde(default)]
    created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
struct GhLicenseJson {
    spdx_id: Option<String>,
    key: Option<String>,
    name: Option<String>,
}

// ---------------------------------------------------------------------------
// Params parsing
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct ParsedDiscover {
    user_q: String,
    language: Option<String>,
    license: Option<String>,
    min_stars: Option<u32>,
    exclude_archived: bool,
    active_within_days: Option<u32>,
    sort: String,
    page: u32,
}

fn parse_params(p: &DiscoverSearchParams) -> Result<ParsedDiscover, &'static str> {
    let user_q = p.q.as_deref().unwrap_or("").trim().to_string();
    if user_q.len() > MAX_USER_Q_LEN {
        return Err("q must be at most 256 characters");
    }

    let language = p
        .language
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let license = p
        .license
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let min_stars = match p.min_stars.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => None,
        Some(raw) => {
            let n: u32 = raw
                .parse()
                .map_err(|_| "min_stars must be a non-negative integer")?;
            Some(n)
        }
    };

    let exclude_archived = parse_exclude_archived(p.exclude_archived.as_deref());

    let active_within_days = match parse_active_within(p.active_within.as_deref()) {
        Ok(None) => None,
        Ok(Some(n)) => Some(n as u32),
        Err(e) => return Err(e),
    };

    let sort = match p.sort.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None | Some("stars") => "stars".to_string(),
        Some("updated") => "updated".to_string(),
        Some(_) => return Err("sort must be stars|updated"),
    };

    let page = match p.page.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => 1u32,
        Some(raw) => {
            let n: u32 = raw.parse().map_err(|_| "page must be an integer 1..=10")?;
            if !(1..=MAX_PAGE).contains(&n) {
                return Err("page must be an integer 1..=10");
            }
            n
        }
    };

    // At least one search condition (exclude_archived alone does not count).
    let has_condition = !user_q.is_empty()
        || language.is_some()
        || license.is_some()
        || min_stars.is_some()
        || active_within_days.is_some();
    if !has_condition {
        return Err("at least one of q, language, license, min_stars, active_within is required");
    }

    Ok(ParsedDiscover {
        user_q,
        language,
        license,
        min_stars,
        exclude_archived,
        active_within_days,
        sort,
        page,
    })
}

// ---------------------------------------------------------------------------
// Token resolution
// ---------------------------------------------------------------------------

struct ResolvedToken {
    token: String,
    auth_mode: AuthMode,
}

async fn resolve_token(
    state: &AppState,
    user_id: i64,
) -> Result<ResolvedToken, Response> {
    match users::get_user_github_token_ciphertext(&state.pool, user_id).await {
        Ok(Some(blob)) => match crypto::decrypt_token(&state.settings.token_encryption_key, &blob)
        {
            Ok(token) => {
                return Ok(ResolvedToken {
                    token,
                    auth_mode: AuthMode::User,
                });
            }
            Err(e) => {
                tracing::error!(error = %e, user_id, "failed to decrypt user github token");
                return Err(StatusCode::INTERNAL_SERVER_ERROR.into_response());
            }
        },
        Ok(None) => {}
        Err(e) => {
            tracing::error!(error = %e, "get_user_github_token_ciphertext failed");
            return Err(StatusCode::INTERNAL_SERVER_ERROR.into_response());
        }
    }

    if let Some(token) = state
        .settings
        .github_token
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Ok(ResolvedToken {
            token: token.to_string(),
            auth_mode: AuthMode::Shared,
        });
    }

    Err((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({ "error": "github_token_required" })),
    )
        .into_response())
}

// ---------------------------------------------------------------------------
// Rate limit
// ---------------------------------------------------------------------------

fn rate_limit_response(err: RateLimitError, auth_mode: AuthMode) -> Response {
    let scope = match err.scope {
        RateLimitScope::Global => "global",
        RateLimitScope::User => "user",
    };
    let mut resp = (
        StatusCode::TOO_MANY_REQUESTS,
        Json(serde_json::json!({
            "error": "rate_limited",
            "scope": scope,
            "auth_mode": auth_mode.as_str(),
            "retry_after_secs": err.retry_after_secs,
        })),
    )
        .into_response();
    if let Ok(v) = HeaderValue::from_str(&err.retry_after_secs.to_string()) {
        resp.headers_mut().insert(axum::http::header::RETRY_AFTER, v);
    }
    resp
}

fn check_rate_limit(
    state: &AppState,
    user_id: i64,
    auth_mode: AuthMode,
) -> Result<(), RateLimitError> {
    match auth_mode {
        AuthMode::User => state.discover_rate_limiters.check_user_token(user_id),
        AuthMode::Shared => state.discover_rate_limiters.check_shared(user_id),
    }
}

// ---------------------------------------------------------------------------
// GitHub call
// ---------------------------------------------------------------------------

enum GhCallError {
    AuthFailed,
    RateLimited,
    Upstream(String),
    Network(String),
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("gh-trending-api/0.1")
        .build()
        .expect("reqwest client")
}

async fn call_github_search(
    api_base: &str,
    token: &str,
    q: &str,
    sort: &str,
    page: u32,
) -> Result<GhSearchResponse, GhCallError> {
    let client = http_client();
    let base = api_base.trim_end_matches('/');
    let url = format!("{base}/search/repositories");
    let page_s = page.to_string();
    let per_page_s = PER_PAGE.to_string();

    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .bearer_auth(token)
        .query(&[
            ("q", q),
            ("sort", sort),
            ("order", "desc"),
            ("per_page", per_page_s.as_str()),
            ("page", page_s.as_str()),
        ])
        .send()
        .await
        .map_err(|e| GhCallError::Network(e.to_string()))?;

    let status = resp.status();
    if status.is_success() {
        return resp
            .json::<GhSearchResponse>()
            .await
            .map_err(|e| GhCallError::Upstream(format!("decode: {e}")));
    }

    let body = resp.text().await.unwrap_or_default();
    match status.as_u16() {
        401 | 403 => {
            tracing::warn!(%status, "github search auth failed");
            Err(GhCallError::AuthFailed)
        }
        429 => {
            tracing::warn!("github search rate limited");
            Err(GhCallError::RateLimited)
        }
        _ => {
            tracing::warn!(%status, body_len = body.len(), "github search upstream error");
            Err(GhCallError::Upstream(format!("status {status}")))
        }
    }
}

fn gh_error_response(err: GhCallError) -> Response {
    match err {
        GhCallError::AuthFailed => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": "github_auth_failed" })),
        )
            .into_response(),
        GhCallError::RateLimited => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({ "error": "github_rate_limited" })),
        )
            .into_response(),
        GhCallError::Network(msg) => {
            tracing::warn!(error = %msg, "github search network error");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": "bad_gateway" })),
            )
                .into_response()
        }
        GhCallError::Upstream(msg) => {
            tracing::warn!(error = %msg, "github search upstream error");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": "bad_gateway" })),
            )
                .into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

async fn search(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
    Query(params): Query<DiscoverSearchParams>,
) -> Response {
    let parsed = match parse_params(&params) {
        Ok(p) => p,
        Err(msg) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response();
        }
    };

    let resolved = match resolve_token(&state, claims.sub).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    // Rate limit AFTER validation + token resolution, BEFORE GitHub.
    if let Err(e) = check_rate_limit(&state, claims.sub, resolved.auth_mode) {
        return rate_limit_response(e, resolved.auth_mode);
    }

    let now = Utc::now();
    let q = build_discover_q(
        &parsed.user_q,
        parsed.language.as_deref(),
        parsed.license.as_deref(),
        parsed.min_stars,
        parsed.exclude_archived,
        parsed.active_within_days,
        now,
    );

    let gh = match call_github_search(
        &state.settings.github_api_base,
        &resolved.token,
        &q,
        &parsed.sort,
        parsed.page,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => return gh_error_response(e),
    };

    let names: Vec<String> = gh.items.iter().map(|i| i.full_name.clone()).collect();

    let local_index: HashSet<String> =
        match store::repos_exist_full_names(&state.pool, &names).await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, "repos_exist_full_names failed");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };

    let tracked: HashSet<String> = match tracked_full_names(&state.pool, claims.sub).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "tracked_full_names failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let items: Vec<DiscoverItem> = gh
        .items
        .into_iter()
        .map(|it| {
            let license = it
                .license
                .and_then(|l| license_from_gh(l.spdx_id, l.key, l.name));
            let health = compute_health(it.archived, it.pushed_at, now)
                .as_str()
                .to_string();
            let full_name = it.full_name;
            DiscoverItem {
                already_tracked: tracked.contains(&full_name),
                in_local_index: local_index.contains(&full_name),
                full_name,
                html_url: it.html_url,
                description: it.description,
                language: it.language,
                license,
                stars: it.stargazers_count,
                forks: it.forks_count,
                topics: it.topics,
                pushed_at: it.pushed_at,
                archived: it.archived,
                open_issues_count: it.open_issues_count,
                created_at_gh: it.created_at,
                latest_release_at: None,
                health,
            }
        })
        .collect();

    (
        StatusCode::OK,
        Json(DiscoverSearchResp {
            items,
            page: parsed.page,
            per_page: PER_PAGE,
            total_count: gh.total_count,
            incomplete_results: gh.incomplete_results,
            auth_mode: resolved.auth_mode,
        }),
    )
        .into_response()
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
    use chrono::TimeZone;
    use ght_core::models::RepoInput;
    use ght_core::{db, store as core_store, users};
    use serial_test::serial;
    use tower::ServiceExt;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // ---- build_discover_q unit tests ----

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 7, 12, 0, 0).unwrap()
    }

    #[test]
    fn build_q_language_license_stars_archived_pushed() {
        let q = build_discover_q(
            "http",
            Some("Rust"),
            Some("mit"),
            Some(100),
            true,
            Some(90),
            fixed_now(),
        );
        assert_eq!(
            q,
            "http language:Rust license:mit stars:>=100 archived:false pushed:>2026-05-09"
        );
    }

    #[test]
    fn build_q_empty_user_q_still_emits_qualifiers() {
        let q = build_discover_q("", Some("Go"), None, None, true, None, fixed_now());
        assert_eq!(q, "language:Go archived:false");
    }

    #[test]
    fn build_q_no_archived_when_false() {
        let q = build_discover_q("cli", None, None, Some(0), false, None, fixed_now());
        assert_eq!(q, "cli stars:>=0");
        assert!(!q.contains("archived"));
    }

    #[test]
    fn build_q_quotes_language_with_space() {
        let q = build_discover_q(
            "",
            Some("Visual Basic"),
            None,
            None,
            false,
            None,
            fixed_now(),
        );
        assert_eq!(q, "language:\"Visual Basic\"");
    }

    #[test]
    fn build_q_trims_user_q_and_skips_blank_language() {
        let q = build_discover_q(
            "  widget  ",
            Some("  "),
            Some("apache-2.0"),
            None,
            false,
            None,
            fixed_now(),
        );
        assert_eq!(q, "widget license:apache-2.0");
    }

    #[test]
    fn build_q_active_within_one_day() {
        let q = build_discover_q("", None, None, None, false, Some(1), fixed_now());
        assert_eq!(q, "pushed:>2026-08-06");
    }

    #[test]
    fn build_q_all_empty_is_empty_string() {
        let q = build_discover_q("", None, None, None, false, None, fixed_now());
        assert_eq!(q, "");
    }

    #[test]
    fn parse_params_requires_at_least_one_condition() {
        let p = DiscoverSearchParams {
            q: Some("  ".into()),
            language: None,
            license: None,
            min_stars: None,
            exclude_archived: Some("1".into()),
            active_within: None,
            sort: None,
            page: None,
        };
        let err = parse_params(&p).unwrap_err();
        assert!(err.contains("at least one"));
    }

    #[test]
    fn parse_params_accepts_language_only() {
        let p = DiscoverSearchParams {
            q: None,
            language: Some("Rust".into()),
            license: None,
            min_stars: None,
            exclude_archived: None,
            active_within: None,
            sort: Some("updated".into()),
            page: Some("2".into()),
        };
        let ok = parse_params(&p).unwrap();
        assert_eq!(ok.language.as_deref(), Some("Rust"));
        assert_eq!(ok.sort, "updated");
        assert_eq!(ok.page, 2);
        assert!(ok.exclude_archived);
    }

    #[test]
    fn parse_params_rejects_bad_page_and_sort() {
        let mut p = DiscoverSearchParams {
            q: Some("x".into()),
            language: None,
            license: None,
            min_stars: None,
            exclude_archived: None,
            active_within: None,
            sort: Some("forks".into()),
            page: None,
        };
        assert!(parse_params(&p).unwrap_err().contains("sort"));
        p.sort = Some("stars".into());
        p.page = Some("11".into());
        assert!(parse_params(&p).unwrap_err().contains("page"));
        p.page = Some("0".into());
        assert!(parse_params(&p).unwrap_err().contains("page"));
    }

    // ---- Integration helpers ----

    async fn test_state(
        github_base: Option<&str>,
        github_token: Option<&str>,
        rate_limits: Option<(u32, u32, u32)>,
    ) -> AppState {
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
        let tok = github_token.map(|s| s.to_string());
        let rl = rate_limits;
        let settings = ght_core::config::Settings::from_map(|k| match k {
            "DATABASE_URL" => Some(url.clone()),
            "JWT_SECRET" => Some("test-secret".into()),
            "GITHUB_API_BASE" => base.clone(),
            "GITHUB_TOKEN" => tok.clone(),
            "DISCOVER_RATE_LIMIT_PER_MIN" => rl.map(|(g, _, _)| g.to_string()),
            "DISCOVER_RATE_LIMIT_PER_USER_PER_MIN" => rl.map(|(_, u, _)| u.to_string()),
            "DISCOVER_RATE_LIMIT_PER_USER_WITH_TOKEN_PER_MIN" => rl.map(|(_, _, ut)| ut.to_string()),
            _ => None,
        })
        .unwrap();
        AppState::new(pool, settings)
    }

    async fn create_user(state: &AppState, username: &str) -> i64 {
        users::create_user(&state.pool, username, "hash", None)
            .await
            .unwrap()
    }

    fn auth_cookie(state: &AppState, user_id: i64, username: &str) -> String {
        let jwt = tokens::issue_access(&state.settings.jwt_secret, user_id, username).unwrap();
        format!("access_token={jwt}")
    }

    async fn call(
        state: AppState,
        method: &str,
        uri: &str,
        cookie: Option<&str>,
    ) -> (StatusCode, String, Option<String>) {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(c) = cookie {
            builder = builder.header("cookie", c);
        }
        let resp = crate::build_router(state)
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let retry_after = resp
            .headers()
            .get(axum::http::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap(), retry_after)
    }

    fn search_item_json(full_name: &str, stars: i32, pushed_at: &str, archived: bool) -> String {
        format!(
            r#"{{
              "full_name":"{full_name}",
              "html_url":"https://github.com/{full_name}",
              "description":"desc {full_name}",
              "language":"Rust",
              "license":{{"spdx_id":"MIT","key":"mit","name":"MIT License"}},
              "stargazers_count":{stars},
              "forks_count":10,
              "topics":["http","cli"],
              "pushed_at":"{pushed_at}",
              "archived":{archived},
              "open_issues_count":3,
              "created_at":"2019-03-01T00:00:00Z"
            }}"#,
            archived = if archived { "true" } else { "false" }
        )
    }

    // ---- Integration tests ----

    #[tokio::test]
    #[serial]
    async fn search_shared_token_200_maps_items_and_flags() {
        let server = MockServer::start().await;
        let body = format!(
            r#"{{"total_count":2,"incomplete_results":false,"items":[{},{}]}}"#,
            search_item_json("local/indexed", 100, "2026-08-01T00:00:00Z", false),
            search_item_json("remote/only", 50, "2025-01-01T00:00:00Z", false),
        );
        Mock::given(method("GET"))
            .and(path("/search/repositories"))
            .and(query_param("q", "language:Rust archived:false"))
            .and(query_param("sort", "stars"))
            .and(query_param("order", "desc"))
            .and(query_param("per_page", "30"))
            .and(query_param("page", "1"))
            .and(header("authorization", "Bearer shared_tok"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .expect(1)
            .mount(&server)
            .await;

        let state = test_state(Some(&server.uri()), Some("shared_tok"), None).await;
        let uid = create_user(&state, "disc_user").await;
        let cookie = auth_cookie(&state, uid, "disc_user");

        // Seed local index + track only local/indexed
        let date = chrono::NaiveDate::from_ymd_opt(2026, 8, 7).unwrap();
        let rid = core_store::upsert_repo(
            &state.pool,
            &RepoInput {
                full_name: "local/indexed".into(),
                owner: "local".into(),
                name: "indexed".into(),
                html_url: "https://github.com/local/indexed".into(),
                language: Some("Rust".into()),
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
            },
            date,
        )
        .await
        .unwrap();
        core_store::track_repo(&state.pool, uid, rid).await.unwrap();

        let (status, body, _) = call(
            state.clone(),
            "GET",
            "/api/discover/search?language=Rust",
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body={body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["auth_mode"], "shared");
        assert_eq!(v["page"], 1);
        assert_eq!(v["per_page"], 30);
        assert_eq!(v["total_count"], 2);
        assert_eq!(v["items"].as_array().unwrap().len(), 2);

        let a = &v["items"][0];
        assert_eq!(a["full_name"], "local/indexed");
        assert_eq!(a["license"], "MIT");
        assert_eq!(a["stars"], 100);
        assert_eq!(a["latest_release_at"], serde_json::Value::Null);
        assert_eq!(a["already_tracked"], true);
        assert_eq!(a["in_local_index"], true);
        assert_eq!(a["health"], "active");
        assert_eq!(a["topics"][0], "http");

        let b = &v["items"][1];
        assert_eq!(b["full_name"], "remote/only");
        assert_eq!(b["already_tracked"], false);
        assert_eq!(b["in_local_index"], false);
        assert_eq!(b["health"], "stale");
    }

    #[tokio::test]
    #[serial]
    async fn search_user_pat_preferred_over_shared() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search/repositories"))
            .and(header("authorization", "Bearer ghp_user_pat_xyz"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"total_count":0,"incomplete_results":false,"items":[]}"#,
            ))
            .expect(1)
            .mount(&server)
            .await;

        let state = test_state(Some(&server.uri()), Some("shared_tok"), None).await;
        let uid = create_user(&state, "pat_pref").await;
        let cookie = auth_cookie(&state, uid, "pat_pref");

        let blob = crypto::encrypt_token(
            &state.settings.token_encryption_key,
            "ghp_user_pat_xyz",
        )
        .unwrap();
        users::set_user_github_token(&state.pool, uid, &blob)
            .await
            .unwrap();

        let (status, body, _) = call(
            state,
            "GET",
            "/api/discover/search?q=tokio",
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body={body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["auth_mode"], "user");
        assert_eq!(v["items"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    #[serial]
    async fn search_503_without_any_token() {
        let state = test_state(None, None, None).await;
        let uid = create_user(&state, "no_tok").await;
        let cookie = auth_cookie(&state, uid, "no_tok");

        let (status, body, _) = call(
            state,
            "GET",
            "/api/discover/search?language=Rust",
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "body={body}");
        assert!(body.contains("github_token_required"));
    }

    #[tokio::test]
    #[serial]
    async fn search_429_app_rate_limit_shared_before_github() {
        let server = MockServer::start().await;
        // Should never be called on the limited request.
        Mock::given(method("GET"))
            .and(path("/search/repositories"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"total_count":0,"incomplete_results":false,"items":[]}"#,
            ))
            .expect(1) // only first request
            .mount(&server)
            .await;

        // global=1 so second shared request is rate limited without hitting GitHub again.
        let state = test_state(Some(&server.uri()), Some("shared_tok"), Some((1, 10, 25))).await;
        let uid = create_user(&state, "rl_user").await;
        let cookie = auth_cookie(&state, uid, "rl_user");

        let (status1, body1, _) = call(
            state.clone(),
            "GET",
            "/api/discover/search?q=one",
            Some(&cookie),
        )
        .await;
        assert_eq!(status1, StatusCode::OK, "body={body1}");

        let (status2, body2, retry) = call(
            state,
            "GET",
            "/api/discover/search?q=two",
            Some(&cookie),
        )
        .await;
        assert_eq!(status2, StatusCode::TOO_MANY_REQUESTS, "body={body2}");
        let v: serde_json::Value = serde_json::from_str(&body2).unwrap();
        assert_eq!(v["error"], "rate_limited");
        assert_eq!(v["scope"], "global");
        assert_eq!(v["auth_mode"], "shared");
        assert!(v["retry_after_secs"].as_u64().unwrap() >= 1);
        assert!(retry.is_some());
    }

    #[tokio::test]
    #[serial]
    async fn search_400_empty_conditions_and_401_unauth() {
        let state = test_state(None, Some("tok"), None).await;
        let uid = create_user(&state, "bad_q").await;
        let cookie = auth_cookie(&state, uid, "bad_q");

        let (status, body, _) = call(
            state.clone(),
            "GET",
            "/api/discover/search?exclude_archived=1",
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body={body}");
        assert!(body.contains("at least one"));

        let (status, _, _) = call(
            state,
            "GET",
            "/api/discover/search?language=Rust",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    #[serial]
    async fn search_github_429_maps_to_github_rate_limited() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search/repositories"))
            .respond_with(ResponseTemplate::new(429).set_body_string(r#"{"message":"API rate limit exceeded"}"#))
            .expect(1)
            .mount(&server)
            .await;

        let state = test_state(Some(&server.uri()), Some("shared_tok"), None).await;
        let uid = create_user(&state, "gh_rl").await;
        let cookie = auth_cookie(&state, uid, "gh_rl");

        let (status, body, _) = call(
            state,
            "GET",
            "/api/discover/search?q=foo",
            Some(&cookie),
        )
        .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "body={body}");
        assert!(body.contains("github_rate_limited"));
    }
}
