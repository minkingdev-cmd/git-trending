use chrono::NaiveDate;
use ght_core::models::{Board, RepoInput, SnapshotInput};
use ght_core::store as core_store;
use sqlx::PgPool;

#[derive(Debug, Clone)]
pub struct TopEntry {
    pub repo_full_name: String,
    pub html_url: String,
    pub description: Option<String>,
    pub language: Option<String>,
    pub stars: i32,
    pub forks: i32,
    pub watchers: Option<i32>,
}

pub fn split_full_name(full_name: &str) -> (&str, &str) {
    full_name.split_once('/').unwrap_or((full_name, ""))
}

async fn store_one(
    pool: &PgPool,
    date: NaiveDate,
    board: Board,
    full_name: &str,
    html_url: &str,
    description: &Option<String>,
    language: &Option<String>,
    snap: SnapshotInput,
) -> anyhow::Result<()> {
    let (owner, name) = split_full_name(full_name);
    let repo = RepoInput {
        full_name: full_name.to_string(),
        owner: owner.to_string(),
        name: name.to_string(),
        html_url: html_url.to_string(),
        language: language.clone(),
        description: description.clone(),
    };
    let repo_id = core_store::upsert_repo(pool, &repo, date).await?;
    core_store::upsert_snapshot(pool, repo_id, date, board, &snap).await?;
    Ok(())
}

pub async fn store_top_rows(pool: &PgPool, date: NaiveDate, board: Board, rows: &[TopEntry]) -> anyhow::Result<usize> {
    let mut n = 0;
    for r in rows {
        store_one(
            pool,
            date,
            board,
            &r.repo_full_name,
            &r.html_url,
            &r.description,
            &r.language,
            SnapshotInput { stars: r.stars, forks: r.forks, watchers: r.watchers, stars_today: None },
        )
        .await?;
        n += 1;
    }
    Ok(n)
}

pub async fn store_trending_rows(pool: &PgPool, date: NaiveDate, rows: &[crate::trending::TrendingRepo]) -> anyhow::Result<usize> {
    let mut n = 0;
    for t in rows {
        store_one(
            pool,
            date,
            Board::TrendingDaily,
            &t.full_name,
            &format!("https://github.com/{}", t.full_name),
            &t.description,
            &t.language,
            SnapshotInput { stars: t.stars, forks: t.forks, watchers: None, stars_today: Some(t.stars_today) },
        )
        .await?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_full_name_handles_missing_slash() {
        assert_eq!(split_full_name("a/b"), ("a", "b"));
        assert_eq!(split_full_name("lonely"), ("lonely", ""));
    }
}
