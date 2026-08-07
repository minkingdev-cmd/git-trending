use chrono::NaiveDate;
use ght_core::models::{Board, RepoInput, SnapshotInput};
use ght_core::store as core_store;
use sqlx::PgPool;

use crate::enrich;

#[derive(Debug, Clone)]
pub struct TopEntry {
    pub repo_full_name: String,
    pub html_url: String,
    pub description: Option<String>,
    pub language: Option<String>,
    pub license: Option<String>,
    pub stars: i32,
    pub forks: i32,
    pub watchers: Option<i32>,
    pub topics: Vec<String>,
}

/// Repo row that still needs topics, language shares, and/or license filled in.
#[derive(Debug, Clone)]
pub struct EnrichmentNeed {
    pub full_name: String,
    pub owner: String,
    pub name: String,
    pub html_url: String,
    pub language: Option<String>,
    pub description: Option<String>,
    pub needs_topics: bool,
    pub needs_languages: bool,
    pub needs_license: bool,
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
    license: &Option<String>,
    topics: &[String],
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
        license: license.clone(),
        // Empty languages here: core upsert preserves existing; enrich phase fills later.
        topics: enrich::normalize_topics(topics),
        languages_json: RepoInput::languages_empty(),
        language_names: vec![],
        pushed_at: None,
        archived: false,
        open_issues_count: None,
        created_at_gh: None,
        latest_release_at: None,
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
            &r.license,
            &r.topics,
            SnapshotInput {
                stars: r.stars,
                forks: r.forks,
                watchers: r.watchers,
                stars_today: None,
            },
        )
        .await?;
        n += 1;
    }
    Ok(n)
}

pub async fn store_trending_rows(pool: &PgPool, date: NaiveDate, rows: &[crate::trending::TrendingRepo]) -> anyhow::Result<usize> {
    let mut n = 0;
    for t in rows {
        let no_license = None;
        store_one(
            pool,
            date,
            Board::TrendingDaily,
            &t.full_name,
            &format!("https://github.com/{}", t.full_name),
            &t.description,
            &t.language,
            &no_license,
            &[],
            SnapshotInput {
                stars: t.stars,
                forks: t.forks,
                watchers: None,
                stars_today: Some(t.stars_today),
            },
        )
        .await?;
        n += 1;
    }
    Ok(n)
}

/// Repos on today's boards missing topics, language_names, and/or license.
pub async fn list_repos_needing_enrichment(
    pool: &PgPool,
    date: NaiveDate,
) -> Result<Vec<EnrichmentNeed>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT DISTINCT
               r.full_name AS "full_name!",
               r.owner AS "owner!",
               r.name AS "name!",
               r.html_url AS "html_url!",
               r.language,
               r.description,
               (cardinality(r.topics) = 0) AS "needs_topics!",
               (cardinality(r.language_names) = 0) AS "needs_languages!",
               (r.license IS NULL OR btrim(r.license) = '') AS "needs_license!"
           FROM repos r
           JOIN snapshots s ON s.repo_id = r.id
           WHERE s.snapshot_date = $1
             AND (
               cardinality(r.topics) = 0
               OR cardinality(r.language_names) = 0
               OR r.license IS NULL
               OR btrim(r.license) = ''
             )
           ORDER BY r.full_name"#,
        date
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| EnrichmentNeed {
            full_name: r.full_name,
            owner: r.owner,
            name: r.name,
            html_url: r.html_url,
            language: r.language,
            description: r.description,
            needs_topics: r.needs_topics,
            needs_languages: r.needs_languages,
            needs_license: r.needs_license,
        })
        .collect())
}

/// Write enrichment fields for an existing repo (empty fields preserve prior values via upsert).
/// Returns the repo id.
pub async fn apply_enrichment(
    pool: &PgPool,
    date: NaiveDate,
    need: &EnrichmentNeed,
    topics: Vec<String>,
    language_names: Vec<String>,
    languages_json: serde_json::Value,
    license: Option<String>,
) -> Result<i64, sqlx::Error> {
    let repo = RepoInput {
        full_name: need.full_name.clone(),
        owner: need.owner.clone(),
        name: need.name.clone(),
        html_url: need.html_url.clone(),
        language: need.language.clone(),
        description: need.description.clone(),
        // None preserves existing via COALESCE; Some fills missing / upgrades blank.
        license,
        topics,
        languages_json,
        language_names,
        pushed_at: None,
        archived: false,
        open_issues_count: None,
        created_at_gh: None,
        latest_release_at: None,
    };
    core_store::upsert_repo(pool, &repo, date).await
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
