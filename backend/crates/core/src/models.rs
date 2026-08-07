use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Board {
    TrendingDaily,
    TopStars,
    TopForks,
    TopWatchers,
    TrackedDaily,
}

impl Board {
    pub fn as_str(self) -> &'static str {
        match self {
            Board::TrendingDaily => "trending_daily",
            Board::TopStars => "top_stars",
            Board::TopForks => "top_forks",
            Board::TopWatchers => "top_watchers",
            Board::TrackedDaily => "tracked_daily",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "trending_daily" => Some(Board::TrendingDaily),
            "top_stars" => Some(Board::TopStars),
            "top_forks" => Some(Board::TopForks),
            "top_watchers" => Some(Board::TopWatchers),
            "tracked_daily" => Some(Board::TrackedDaily),
            _ => None,
        }
    }
}

/// Topic multi-select mode for leaderboard filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TopicMode {
    #[default]
    And,
    Or,
}

impl TopicMode {
    pub fn as_str(self) -> &'static str {
        match self {
            TopicMode::And => "and",
            TopicMode::Or => "or",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "and" => Some(TopicMode::And),
            "or" => Some(TopicMode::Or),
            _ => None,
        }
    }
}

/// Language share for API serialization; DB JSON may also store `bytes`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LanguageShare {
    pub name: String,
    pub pct: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<i64>,
}

/// Filters applied to leaderboard list queries (after board/date).
/// Empty slices and blank `q` are treated as no filter by the store.
#[derive(Debug, Clone, Copy)]
pub struct LeaderboardFilter<'a> {
    /// Legacy primary-language equality (`repos.language`).
    pub language: Option<&'a str>,
    /// Multi-language OR filter against `repos.language_names` or primary language.
    pub languages: Option<&'a [String]>,
    /// Multi-license OR filter against `repos.license`.
    pub licenses: Option<&'a [String]>,
    pub topics: Option<&'a [String]>,
    pub topic_mode: TopicMode,
    pub q: Option<&'a str>,
    /// When true, drop archived repos (`repos.archived = false`).
    /// Store default is false (no filter); public API may pass true later.
    pub exclude_archived: bool,
    /// When `Some(n)`, require non-archived and `pushed_at >= now() - n days`.
    pub active_within_days: Option<i32>,
}

impl Default for LeaderboardFilter<'_> {
    fn default() -> Self {
        Self {
            language: None,
            languages: None,
            licenses: None,
            topics: None,
            topic_mode: TopicMode::And,
            q: None,
            exclude_archived: false,
            active_within_days: None,
        }
    }
}

impl LeaderboardFilter<'_> {
    pub fn empty() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone)]
pub struct HistoryPoint {
    pub snapshot_date: chrono::NaiveDate,
    pub stars: i32,
    pub forks: i32,
    pub watchers: Option<i32>,
    pub stars_today: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct RepoInput {
    pub full_name: String,
    pub owner: String,
    pub name: String,
    pub html_url: String,
    pub language: Option<String>,
    pub description: Option<String>,
    /// SPDX id or short key (e.g. MIT, Apache-2.0); None if unknown.
    pub license: Option<String>,
    pub topics: Vec<String>,
    /// JSON array of language shares (name/pct/bytes); stored in `repos.languages`.
    pub languages_json: serde_json::Value,
    pub language_names: Vec<String>,
    /// Last push time from GitHub (health).
    pub pushed_at: Option<chrono::DateTime<chrono::Utc>>,
    /// GitHub archived flag (health). Default false for board-only upserts.
    pub archived: bool,
    pub open_issues_count: Option<i32>,
    /// Repo creation time on GitHub.
    pub created_at_gh: Option<chrono::DateTime<chrono::Utc>>,
    pub latest_release_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl RepoInput {
    pub fn languages_empty() -> serde_json::Value {
        serde_json::json!([])
    }
}

#[derive(Debug, Clone)]
pub struct SnapshotInput {
    pub stars: i32,
    pub forks: i32,
    pub watchers: Option<i32>,
    pub stars_today: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct LeaderboardRow {
    pub rank: i64,
    pub full_name: String,
    pub html_url: String,
    pub description: Option<String>,
    pub language: Option<String>,
    pub license: Option<String>,
    pub topics: Vec<String>,
    pub languages: serde_json::Value,
    pub stars: i32,
    pub forks: i32,
    pub watchers: Option<i32>,
    pub stars_today: Option<i32>,
    pub pushed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub archived: bool,
    pub open_issues_count: Option<i32>,
    pub created_at_gh: Option<chrono::DateTime<chrono::Utc>>,
    pub latest_release_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Per-user track cap enforced by API (409 when `count_tracked` >= this).
pub const TRACKED_REPO_LIMIT: i64 = 50;

/// One row from `list_tracked` (repo metadata + best-effort metrics + add time).
#[derive(Debug, Clone)]
pub struct TrackedRow {
    pub repo_id: i64,
    pub full_name: String,
    pub html_url: String,
    pub description: Option<String>,
    pub language: Option<String>,
    pub license: Option<String>,
    pub topics: Vec<String>,
    pub languages: serde_json::Value,
    /// Prefer latest `tracked_daily` snapshot, else any board latest; None if none.
    pub stars: Option<i32>,
    pub forks: Option<i32>,
    pub watchers: Option<i32>,
    pub stars_today: Option<i32>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub pushed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub archived: bool,
    pub open_issues_count: Option<i32>,
    pub created_at_gh: Option<chrono::DateTime<chrono::Utc>>,
    pub latest_release_at: Option<chrono::DateTime<chrono::Utc>>,
}
