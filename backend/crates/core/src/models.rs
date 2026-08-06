#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Board {
    TrendingDaily,
    TopStars,
    TopForks,
    TopWatchers,
}

impl Board {
    pub fn as_str(self) -> &'static str {
        match self {
            Board::TrendingDaily => "trending_daily",
            Board::TopStars => "top_stars",
            Board::TopForks => "top_forks",
            Board::TopWatchers => "top_watchers",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "trending_daily" => Some(Board::TrendingDaily),
            "top_stars" => Some(Board::TopStars),
            "top_forks" => Some(Board::TopForks),
            "top_watchers" => Some(Board::TopWatchers),
            _ => None,
        }
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
    pub stars: i32,
    pub forks: i32,
    pub watchers: Option<i32>,
    pub stars_today: Option<i32>,
}
