use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;

use crate::models::{
    Board, LeaderboardFilter, LeaderboardRow, RepoInput, SnapshotInput, TrackedRow,
};

// Re-export so API/collectors can `use ght_core::store::TRACKED_REPO_LIMIT`.
pub use crate::models::TRACKED_REPO_LIMIT;

/// Bind helpers: empty slices / blank q mean "no filter" (SQL NULL).
fn filter_langs(f: &LeaderboardFilter<'_>) -> Option<Vec<String>> {
    f.languages
        .filter(|a| !a.is_empty())
        .map(|a| a.to_vec())
}

fn filter_licenses(f: &LeaderboardFilter<'_>) -> Option<Vec<String>> {
    f.licenses
        .filter(|a| !a.is_empty())
        .map(|a| a.to_vec())
}

fn filter_topics(f: &LeaderboardFilter<'_>) -> Option<Vec<String>> {
    f.topics.filter(|a| !a.is_empty()).map(|a| a.to_vec())
}

fn filter_q<'a>(f: &LeaderboardFilter<'a>) -> Option<&'a str> {
    f.q.map(str::trim).filter(|s| !s.is_empty())
}

pub async fn upsert_repo(pool: &PgPool, r: &RepoInput, today: NaiveDate) -> Result<i64, sqlx::Error> {
    // Empty topics / languages / language_names do NOT wipe existing enrichment
    // (collector may upsert board rows without re-fetching enrichment every time).
    // Non-empty values overwrite so a successful enrich refresh wins.
    sqlx::query_scalar!(
        r#"INSERT INTO repos (
               full_name, owner, name, html_url, language, description, license, first_seen,
               topics, languages, language_names, last_enriched_at
           )
           VALUES (
               $1, $2, $3, $4, $5, $6, $7, $8,
               $9, $10, $11,
               CASE
                 WHEN cardinality($9::text[]) > 0 OR cardinality($11::text[]) > 0
                 THEN now()
                 ELSE NULL
               END
           )
           ON CONFLICT (full_name) DO UPDATE
           SET html_url = EXCLUDED.html_url,
               language = EXCLUDED.language,
               description = EXCLUDED.description,
               license = COALESCE(EXCLUDED.license, repos.license),
               topics = CASE
                 WHEN cardinality(EXCLUDED.topics) > 0 THEN EXCLUDED.topics
                 ELSE repos.topics
               END,
               languages = CASE
                 WHEN EXCLUDED.languages <> '[]'::jsonb THEN EXCLUDED.languages
                 ELSE repos.languages
               END,
               language_names = CASE
                 WHEN cardinality(EXCLUDED.language_names) > 0 THEN EXCLUDED.language_names
                 ELSE repos.language_names
               END,
               last_enriched_at = CASE
                 WHEN cardinality(EXCLUDED.topics) > 0
                   OR cardinality(EXCLUDED.language_names) > 0
                 THEN now()
                 ELSE repos.last_enriched_at
               END
           RETURNING id"#,
        r.full_name,
        r.owner,
        r.name,
        r.html_url,
        r.language,
        r.description,
        r.license,
        today,
        &r.topics as &[String],
        r.languages_json.clone(),
        &r.language_names as &[String],
    )
    .fetch_one(pool)
    .await
}

pub async fn upsert_snapshot(
    pool: &PgPool,
    repo_id: i64,
    date: NaiveDate,
    board: Board,
    s: &SnapshotInput,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"INSERT INTO snapshots (repo_id, snapshot_date, board, stars, forks, watchers, stars_today)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           ON CONFLICT (repo_id, snapshot_date, board) DO UPDATE
           SET stars = EXCLUDED.stars,
               forks = EXCLUDED.forks,
               watchers = EXCLUDED.watchers,
               stars_today = EXCLUDED.stars_today"#,
        repo_id,
        date,
        board.as_str(),
        s.stars,
        s.forks,
        s.watchers,
        s.stars_today
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Index a (user-added or tracked) repo onto the same metric boards the public
/// leaderboard reads, plus `tracked_daily` for personal history.
///
/// Does **not** write `trending_daily` (no reliable stars_today from REST).
pub async fn upsert_indexed_snapshots(
    pool: &PgPool,
    repo_id: i64,
    date: NaiveDate,
    s: &SnapshotInput,
) -> Result<(), sqlx::Error> {
    for board in [
        Board::TrackedDaily,
        Board::TopStars,
        Board::TopForks,
        Board::TopWatchers,
    ] {
        upsert_snapshot(pool, repo_id, date, board, s).await?;
    }
    Ok(())
}

/// Merge public-board rows with the caller's tracked repos (already filtered),
/// prefer board row when both exist, re-sort by `metric_key`, re-rank 1..n.
pub fn merge_leaderboard_with_tracked(
    board_rows: Vec<LeaderboardRow>,
    tracked_rows: Vec<LeaderboardRow>,
    metric_key: impl Fn(&LeaderboardRow) -> i64,
) -> Vec<LeaderboardRow> {
    use std::collections::HashMap;
    let mut map: HashMap<String, LeaderboardRow> = HashMap::new();
    for r in board_rows {
        map.insert(r.full_name.clone(), r);
    }
    for r in tracked_rows {
        map.entry(r.full_name.clone()).or_insert(r);
    }
    let mut out: Vec<LeaderboardRow> = map.into_values().collect();
    out.sort_by(|a, b| {
        metric_key(b)
            .cmp(&metric_key(a))
            .then_with(|| a.full_name.cmp(&b.full_name))
    });
    for (i, r) in out.iter_mut().enumerate() {
        r.rank = (i + 1) as i64;
    }
    out
}

/// Convert a tracked list row into a leaderboard row (rank filled later).
pub fn tracked_row_to_leaderboard(t: TrackedRow) -> LeaderboardRow {
    LeaderboardRow {
        rank: 0,
        full_name: t.full_name,
        html_url: t.html_url,
        description: t.description,
        language: t.language,
        license: t.license,
        topics: t.topics,
        languages: t.languages,
        stars: t.stars.unwrap_or(0),
        forks: t.forks.unwrap_or(0),
        watchers: t.watchers,
        stars_today: t.stars_today,
        pushed_at: t.pushed_at,
        archived: t.archived,
        open_issues_count: t.open_issues_count,
        created_at_gh: t.created_at_gh,
        latest_release_at: t.latest_release_at,
    }
}

/// Overwrite health columns for an existing repo (enrich / details path).
pub async fn update_repo_health(
    pool: &PgPool,
    repo_id: i64,
    pushed_at: Option<DateTime<Utc>>,
    archived: bool,
    open_issues_count: Option<i32>,
    created_at_gh: Option<DateTime<Utc>>,
    latest_release_at: Option<DateTime<Utc>>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"UPDATE repos
           SET pushed_at = $2,
               archived = $3,
               open_issues_count = $4,
               created_at_gh = $5,
               latest_release_at = $6
           WHERE id = $1"#,
        repo_id,
        pushed_at,
        archived,
        open_issues_count,
        created_at_gh,
        latest_release_at,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Like [`update_repo_health`] but leaves `latest_release_at` unchanged.
///
/// Use when the release fetch failed transiently so a previous successful value is kept.
pub async fn update_repo_health_keep_release(
    pool: &PgPool,
    repo_id: i64,
    pushed_at: Option<DateTime<Utc>>,
    archived: bool,
    open_issues_count: Option<i32>,
    created_at_gh: Option<DateTime<Utc>>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"UPDATE repos
           SET pushed_at = $2,
               archived = $3,
               open_issues_count = $4,
               created_at_gh = $5
           WHERE id = $1"#,
        repo_id,
        pushed_at,
        archived,
        open_issues_count,
        created_at_gh,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Look up `repos.id` by full_name (for health updates after enrich).
pub async fn repo_id_by_full_name(
    pool: &PgPool,
    full_name: &str,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT id AS "id!" FROM repos WHERE full_name = $1"#,
        full_name
    )
    .fetch_optional(pool)
    .await
}

pub async fn top_by_stars(
    pool: &PgPool,
    date: NaiveDate,
    filter: LeaderboardFilter<'_>,
    limit: i64,
) -> Result<Vec<LeaderboardRow>, sqlx::Error> {
    let langs = filter_langs(&filter);
    let licenses = filter_licenses(&filter);
    let topics = filter_topics(&filter);
    let q = filter_q(&filter);
    sqlx::query_as!(
        LeaderboardRow,
        r#"SELECT ROW_NUMBER() OVER (ORDER BY s.stars DESC) AS "rank!",
                  r.full_name AS full_name, r.html_url AS html_url,
                  r.description AS description, r.language AS language,
                  r.license AS license,
                  r.topics AS "topics!", r.languages AS "languages!",
                  s.stars AS stars, s.forks AS forks, s.watchers AS watchers, s.stars_today AS stars_today,
                  r.pushed_at AS pushed_at,
                  r.archived AS "archived!",
                  r.open_issues_count AS open_issues_count,
                  r.created_at_gh AS created_at_gh,
                  r.latest_release_at AS latest_release_at
           FROM snapshots s JOIN repos r ON r.id = s.repo_id
           WHERE s.snapshot_date = $1 AND s.board = 'top_stars'
             AND ($2::text IS NULL OR r.language = $2)
             AND (
               $3::text[] IS NULL
               OR r.language_names && $3
               OR r.language = ANY($3)
             )
             AND ($4::text[] IS NULL OR r.license = ANY($4))
             AND (
               $5::text[] IS NULL
               OR ($6 = 'and' AND r.topics @> $5)
               OR ($6 = 'or' AND r.topics && $5)
             )
             AND (
               $7::text IS NULL
               OR r.full_name ILIKE '%' || $7 || '%'
               OR COALESCE(r.description, '') ILIKE '%' || $7 || '%'
               OR COALESCE(r.license, '') ILIKE '%' || $7 || '%'
               OR EXISTS (
                 SELECT 1 FROM unnest(r.topics) AS t(topic)
                 WHERE t.topic ILIKE '%' || $7 || '%'
               )
               OR EXISTS (
                 SELECT 1 FROM unnest(r.language_names) AS ln(name)
                 WHERE ln.name ILIKE '%' || $7 || '%'
               )
             )
             AND ($9::bool IS NOT TRUE OR r.archived = false)
             AND (
               $10::int IS NULL
               OR (r.archived = false AND r.pushed_at IS NOT NULL
                   AND r.pushed_at >= (now() - ($10::int || ' days')::interval))
             )
           ORDER BY s.stars DESC
           LIMIT $8"#,
        date,
        filter.language,
        langs.as_deref(),
        licenses.as_deref(),
        topics.as_deref(),
        filter.topic_mode.as_str(),
        q,
        limit,
        filter.exclude_archived,
        filter.active_within_days,
    )
    .fetch_all(pool)
    .await
}

pub async fn top_by_forks(
    pool: &PgPool,
    date: NaiveDate,
    filter: LeaderboardFilter<'_>,
    limit: i64,
) -> Result<Vec<LeaderboardRow>, sqlx::Error> {
    let langs = filter_langs(&filter);
    let licenses = filter_licenses(&filter);
    let topics = filter_topics(&filter);
    let q = filter_q(&filter);
    sqlx::query_as!(
        LeaderboardRow,
        r#"SELECT ROW_NUMBER() OVER (ORDER BY s.forks DESC) AS "rank!",
                  r.full_name AS full_name, r.html_url AS html_url,
                  r.description AS description, r.language AS language,
                  r.license AS license,
                  r.topics AS "topics!", r.languages AS "languages!",
                  s.stars AS stars, s.forks AS forks, s.watchers AS watchers, s.stars_today AS stars_today,
                  r.pushed_at AS pushed_at,
                  r.archived AS "archived!",
                  r.open_issues_count AS open_issues_count,
                  r.created_at_gh AS created_at_gh,
                  r.latest_release_at AS latest_release_at
           FROM snapshots s JOIN repos r ON r.id = s.repo_id
           WHERE s.snapshot_date = $1 AND s.board = 'top_forks'
             AND ($2::text IS NULL OR r.language = $2)
             AND (
               $3::text[] IS NULL
               OR r.language_names && $3
               OR r.language = ANY($3)
             )
             AND ($4::text[] IS NULL OR r.license = ANY($4))
             AND (
               $5::text[] IS NULL
               OR ($6 = 'and' AND r.topics @> $5)
               OR ($6 = 'or' AND r.topics && $5)
             )
             AND (
               $7::text IS NULL
               OR r.full_name ILIKE '%' || $7 || '%'
               OR COALESCE(r.description, '') ILIKE '%' || $7 || '%'
               OR COALESCE(r.license, '') ILIKE '%' || $7 || '%'
               OR EXISTS (
                 SELECT 1 FROM unnest(r.topics) AS t(topic)
                 WHERE t.topic ILIKE '%' || $7 || '%'
               )
               OR EXISTS (
                 SELECT 1 FROM unnest(r.language_names) AS ln(name)
                 WHERE ln.name ILIKE '%' || $7 || '%'
               )
             )
             AND ($9::bool IS NOT TRUE OR r.archived = false)
             AND (
               $10::int IS NULL
               OR (r.archived = false AND r.pushed_at IS NOT NULL
                   AND r.pushed_at >= (now() - ($10::int || ' days')::interval))
             )
           ORDER BY s.forks DESC
           LIMIT $8"#,
        date,
        filter.language,
        langs.as_deref(),
        licenses.as_deref(),
        topics.as_deref(),
        filter.topic_mode.as_str(),
        q,
        limit,
        filter.exclude_archived,
        filter.active_within_days,
    )
    .fetch_all(pool)
    .await
}

pub async fn top_by_watchers(
    pool: &PgPool,
    date: NaiveDate,
    filter: LeaderboardFilter<'_>,
    limit: i64,
) -> Result<Vec<LeaderboardRow>, sqlx::Error> {
    let langs = filter_langs(&filter);
    let licenses = filter_licenses(&filter);
    let topics = filter_topics(&filter);
    let q = filter_q(&filter);
    sqlx::query_as!(
        LeaderboardRow,
        r#"SELECT ROW_NUMBER() OVER (ORDER BY s.watchers DESC NULLS LAST) AS "rank!",
                  r.full_name AS full_name, r.html_url AS html_url,
                  r.description AS description, r.language AS language,
                  r.license AS license,
                  r.topics AS "topics!", r.languages AS "languages!",
                  s.stars AS stars, s.forks AS forks, s.watchers AS watchers, s.stars_today AS stars_today,
                  r.pushed_at AS pushed_at,
                  r.archived AS "archived!",
                  r.open_issues_count AS open_issues_count,
                  r.created_at_gh AS created_at_gh,
                  r.latest_release_at AS latest_release_at
           FROM snapshots s JOIN repos r ON r.id = s.repo_id
           WHERE s.snapshot_date = $1 AND s.board = 'top_watchers'
             AND ($2::text IS NULL OR r.language = $2)
             AND (
               $3::text[] IS NULL
               OR r.language_names && $3
               OR r.language = ANY($3)
             )
             AND ($4::text[] IS NULL OR r.license = ANY($4))
             AND (
               $5::text[] IS NULL
               OR ($6 = 'and' AND r.topics @> $5)
               OR ($6 = 'or' AND r.topics && $5)
             )
             AND (
               $7::text IS NULL
               OR r.full_name ILIKE '%' || $7 || '%'
               OR COALESCE(r.description, '') ILIKE '%' || $7 || '%'
               OR COALESCE(r.license, '') ILIKE '%' || $7 || '%'
               OR EXISTS (
                 SELECT 1 FROM unnest(r.topics) AS t(topic)
                 WHERE t.topic ILIKE '%' || $7 || '%'
               )
               OR EXISTS (
                 SELECT 1 FROM unnest(r.language_names) AS ln(name)
                 WHERE ln.name ILIKE '%' || $7 || '%'
               )
             )
             AND ($9::bool IS NOT TRUE OR r.archived = false)
             AND (
               $10::int IS NULL
               OR (r.archived = false AND r.pushed_at IS NOT NULL
                   AND r.pushed_at >= (now() - ($10::int || ' days')::interval))
             )
           ORDER BY s.watchers DESC NULLS LAST
           LIMIT $8"#,
        date,
        filter.language,
        langs.as_deref(),
        licenses.as_deref(),
        topics.as_deref(),
        filter.topic_mode.as_str(),
        q,
        limit,
        filter.exclude_archived,
        filter.active_within_days,
    )
    .fetch_all(pool)
    .await
}

pub async fn trending(
    pool: &PgPool,
    date: NaiveDate,
    filter: LeaderboardFilter<'_>,
    limit: i64,
) -> Result<Vec<LeaderboardRow>, sqlx::Error> {
    let langs = filter_langs(&filter);
    let licenses = filter_licenses(&filter);
    let topics = filter_topics(&filter);
    let q = filter_q(&filter);
    sqlx::query_as!(
        LeaderboardRow,
        r#"SELECT ROW_NUMBER() OVER (ORDER BY s.stars_today DESC NULLS LAST) AS "rank!",
                  r.full_name AS full_name, r.html_url AS html_url,
                  r.description AS description, r.language AS language,
                  r.license AS license,
                  r.topics AS "topics!", r.languages AS "languages!",
                  s.stars AS stars, s.forks AS forks, s.watchers AS watchers, s.stars_today AS stars_today,
                  r.pushed_at AS pushed_at,
                  r.archived AS "archived!",
                  r.open_issues_count AS open_issues_count,
                  r.created_at_gh AS created_at_gh,
                  r.latest_release_at AS latest_release_at
           FROM snapshots s JOIN repos r ON r.id = s.repo_id
           WHERE s.snapshot_date = $1 AND s.board = 'trending_daily'
             AND ($2::text IS NULL OR r.language = $2)
             AND (
               $3::text[] IS NULL
               OR r.language_names && $3
               OR r.language = ANY($3)
             )
             AND ($4::text[] IS NULL OR r.license = ANY($4))
             AND (
               $5::text[] IS NULL
               OR ($6 = 'and' AND r.topics @> $5)
               OR ($6 = 'or' AND r.topics && $5)
             )
             AND (
               $7::text IS NULL
               OR r.full_name ILIKE '%' || $7 || '%'
               OR COALESCE(r.description, '') ILIKE '%' || $7 || '%'
               OR COALESCE(r.license, '') ILIKE '%' || $7 || '%'
               OR EXISTS (
                 SELECT 1 FROM unnest(r.topics) AS t(topic)
                 WHERE t.topic ILIKE '%' || $7 || '%'
               )
               OR EXISTS (
                 SELECT 1 FROM unnest(r.language_names) AS ln(name)
                 WHERE ln.name ILIKE '%' || $7 || '%'
               )
             )
             AND ($9::bool IS NOT TRUE OR r.archived = false)
             AND (
               $10::int IS NULL
               OR (r.archived = false AND r.pushed_at IS NOT NULL
                   AND r.pushed_at >= (now() - ($10::int || ' days')::interval))
             )
           ORDER BY s.stars_today DESC NULLS LAST
           LIMIT $8"#,
        date,
        filter.language,
        langs.as_deref(),
        licenses.as_deref(),
        topics.as_deref(),
        filter.topic_mode.as_str(),
        q,
        limit,
        filter.exclude_archived,
        filter.active_within_days,
    )
    .fetch_all(pool)
    .await
}

pub async fn latest_snapshot_date(pool: &PgPool, board: Board) -> Result<Option<NaiveDate>, sqlx::Error> {
    let rec = sqlx::query!(
        "SELECT MAX(snapshot_date) AS date FROM snapshots WHERE board = $1",
        board.as_str()
    )
    .fetch_one(pool)
    .await?;
    Ok(rec.date)
}

/// Language counts for one snapshot date, scoped to a single board so each
/// repo counts once and only if it appears on that board.
pub async fn languages_with_counts(
    pool: &PgPool,
    date: NaiveDate,
    board: Board,
) -> Result<Vec<(String, i64)>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT r.language AS lang, COUNT(*) AS cnt
           FROM snapshots s JOIN repos r ON r.id = s.repo_id
           WHERE s.snapshot_date = $1 AND s.board = $2 AND r.language IS NOT NULL
           GROUP BY r.language
           ORDER BY cnt DESC"#,
        date,
        board.as_str()
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| Some((r.lang?, r.cnt?)))
        .collect())
}

pub async fn cleanup_expired_refresh_tokens(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let res = sqlx::query!("DELETE FROM refresh_tokens WHERE expires_at < now()")
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

pub async fn board_count(pool: &PgPool, date: NaiveDate, board: Board) -> Result<i64, sqlx::Error> {
    let rec = sqlx::query!(
        "SELECT COUNT(*) AS cnt FROM snapshots WHERE snapshot_date = $1 AND board = $2",
        date,
        board.as_str()
    )
    .fetch_one(pool)
    .await?;
    Ok(rec.cnt.unwrap_or(0))
}

/// All distinct snapshot dates for a board, newest first.
pub async fn list_snapshot_dates(pool: &PgPool, board: Board) -> Result<Vec<NaiveDate>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT DISTINCT snapshot_date AS "snapshot_date!"
           FROM snapshots
           WHERE board = $1
           ORDER BY snapshot_date DESC"#,
        board.as_str()
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.snapshot_date).collect())
}

/// Daily metric history for one repo on one board (oldest first).
pub async fn repo_history(
    pool: &PgPool,
    full_name: &str,
    board: Board,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<crate::models::HistoryPoint>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT s.snapshot_date AS "snapshot_date!",
                  s.stars AS "stars!",
                  s.forks AS "forks!",
                  s.watchers,
                  s.stars_today
           FROM snapshots s
           JOIN repos r ON r.id = s.repo_id
           WHERE r.full_name = $1
             AND s.board = $2
             AND s.snapshot_date >= $3
             AND s.snapshot_date <= $4
           ORDER BY s.snapshot_date ASC"#,
        full_name,
        board.as_str(),
        from,
        to
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| crate::models::HistoryPoint {
            snapshot_date: r.snapshot_date,
            stars: r.stars,
            forks: r.forks,
            watchers: r.watchers,
            stars_today: r.stars_today,
        })
        .collect())
}

// --- user tracked repos ---

/// Insert a track row. Idempotent on `(user_id, repo_id)`.
/// Caller is responsible for enforcing `TRACKED_REPO_LIMIT` via `count_tracked`.
pub async fn track_repo(pool: &PgPool, user_id: i64, repo_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"INSERT INTO user_tracked_repos (user_id, repo_id)
           VALUES ($1, $2)
           ON CONFLICT (user_id, repo_id) DO NOTHING"#,
        user_id,
        repo_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Remove a track by user + repo full_name. Returns true if a row was deleted.
pub async fn untrack_repo(
    pool: &PgPool,
    user_id: i64,
    full_name: &str,
) -> Result<bool, sqlx::Error> {
    let res = sqlx::query!(
        r#"DELETE FROM user_tracked_repos t
           USING repos r
           WHERE t.repo_id = r.id
             AND t.user_id = $1
             AND r.full_name = $2"#,
        user_id,
        full_name
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// How many repos the user currently tracks.
pub async fn count_tracked(pool: &PgPool, user_id: i64) -> Result<i64, sqlx::Error> {
    let rec = sqlx::query!(
        r#"SELECT COUNT(*) AS "cnt!"
           FROM user_tracked_repos
           WHERE user_id = $1"#,
        user_id
    )
    .fetch_one(pool)
    .await?;
    Ok(rec.cnt)
}

/// Whether this user tracks this repo_id.
pub async fn is_tracked(pool: &PgPool, user_id: i64, repo_id: i64) -> Result<bool, sqlx::Error> {
    let rec = sqlx::query!(
        r#"SELECT EXISTS(
               SELECT 1 FROM user_tracked_repos
               WHERE user_id = $1 AND repo_id = $2
           ) AS "exists!""#,
        user_id,
        repo_id
    )
    .fetch_one(pool)
    .await?;
    Ok(rec.exists)
}

/// Distinct full_names of all tracked repos (collector scan).
pub async fn list_all_tracked_full_names(pool: &PgPool) -> Result<Vec<String>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT DISTINCT r.full_name AS "full_name!"
           FROM user_tracked_repos t
           JOIN repos r ON r.id = t.repo_id
           ORDER BY r.full_name"#
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.full_name).collect())
}

/// Which of `full_names` already exist in `repos` (for discover `in_local_index`).
///
/// Empty input → empty set (no query).
pub async fn repos_exist_full_names(
    pool: &PgPool,
    full_names: &[String],
) -> Result<std::collections::HashSet<String>, sqlx::Error> {
    if full_names.is_empty() {
        return Ok(std::collections::HashSet::new());
    }
    let rows = sqlx::query!(
        r#"SELECT full_name AS "full_name!"
           FROM repos
           WHERE full_name = ANY($1)"#,
        full_names
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.full_name).collect())
}

/// List repos tracked by one user, with optional q / topics / languages filter.
/// Metrics prefer latest `tracked_daily` snapshot, else any board's latest snapshot.
pub async fn list_tracked(
    pool: &PgPool,
    user_id: i64,
    filter: LeaderboardFilter<'_>,
) -> Result<Vec<TrackedRow>, sqlx::Error> {
    let langs = filter_langs(&filter);
    let licenses = filter_licenses(&filter);
    let topics = filter_topics(&filter);
    let q = filter_q(&filter);
    sqlx::query_as!(
        TrackedRow,
        r#"SELECT r.id AS "repo_id!",
                  r.full_name AS "full_name!",
                  r.html_url AS "html_url!",
                  r.description,
                  r.language,
                  r.license,
                  r.topics AS "topics!",
                  r.languages AS "languages!",
                  s.stars AS "stars?",
                  s.forks AS "forks?",
                  s.watchers AS "watchers?",
                  s.stars_today AS "stars_today?",
                  t.created_at AS "created_at!",
                  r.pushed_at AS pushed_at,
                  r.archived AS "archived!",
                  r.open_issues_count AS open_issues_count,
                  r.created_at_gh AS created_at_gh,
                  r.latest_release_at AS latest_release_at
           FROM user_tracked_repos t
           JOIN repos r ON r.id = t.repo_id
           LEFT JOIN LATERAL (
               SELECT sn.stars, sn.forks, sn.watchers, sn.stars_today
               FROM snapshots sn
               WHERE sn.repo_id = r.id
               ORDER BY
                 CASE WHEN sn.board = 'tracked_daily' THEN 0 ELSE 1 END,
                 sn.snapshot_date DESC
               LIMIT 1
           ) s ON true
           WHERE t.user_id = $1
             AND ($2::text IS NULL OR r.language = $2)
             AND (
               $3::text[] IS NULL
               OR r.language_names && $3
               OR r.language = ANY($3)
             )
             AND ($4::text[] IS NULL OR r.license = ANY($4))
             AND (
               $5::text[] IS NULL
               OR ($6 = 'and' AND r.topics @> $5)
               OR ($6 = 'or' AND r.topics && $5)
             )
             AND (
               $7::text IS NULL
               OR r.full_name ILIKE '%' || $7 || '%'
               OR COALESCE(r.description, '') ILIKE '%' || $7 || '%'
               OR COALESCE(r.license, '') ILIKE '%' || $7 || '%'
               OR EXISTS (
                 SELECT 1 FROM unnest(r.topics) AS tp(topic)
                 WHERE tp.topic ILIKE '%' || $7 || '%'
               )
               OR EXISTS (
                 SELECT 1 FROM unnest(r.language_names) AS ln(name)
                 WHERE ln.name ILIKE '%' || $7 || '%'
               )
             )
             AND ($8::bool IS NOT TRUE OR r.archived = false)
             AND (
               $9::int IS NULL
               OR (r.archived = false AND r.pushed_at IS NOT NULL
                   AND r.pushed_at >= (now() - ($9::int || ' days')::interval))
             )
           ORDER BY t.created_at DESC"#,
        user_id,
        filter.language,
        langs.as_deref(),
        licenses.as_deref(),
        topics.as_deref(),
        filter.topic_mode.as_str(),
        q,
        filter.exclude_archived,
        filter.active_within_days,
    )
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::models::{Board, LeaderboardFilter, RepoInput, SnapshotInput, TopicMode};
    use chrono::NaiveDate;
    use serial_test::serial;
    use sqlx::PgPool;

    pub async fn test_pool() -> PgPool {
        let url = std::env::var("DATABASE_URL_TEST")
            .unwrap_or_else(|_| "postgres://postgres@localhost:5432/ghtrending_test".into());
        let pool = db::pg_pool(&url).await.expect("test db unreachable; run `make db`");
        db::migrate(&pool).await.unwrap();
        // NOTE: no TRUNCATE — each test uses unique repo names to avoid cross-test
        // interference when cargo test runs in parallel (the default).
        pool
    }

    fn repo(full_name: &str, lang: Option<&str>) -> RepoInput {
        let (owner, name) = full_name.split_once('/').unwrap();
        RepoInput {
            full_name: full_name.into(),
            owner: owner.into(),
            name: name.into(),
            html_url: format!("https://github.com/{full_name}"),
            language: lang.map(String::from),
            description: Some(format!("desc of {full_name}")),
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

    fn repo_with_topics(full_name: &str, lang: Option<&str>, topics: &[&str]) -> RepoInput {
        let mut r = repo(full_name, lang);
        r.topics = topics.iter().map(|t| (*t).to_string()).collect();
        r
    }

    fn lang_filter(language: Option<&str>) -> LeaderboardFilter<'_> {
        LeaderboardFilter {
            language,
            ..LeaderboardFilter::empty()
        }
    }

    #[tokio::test]
    #[serial]
    async fn upsert_repo_is_idempotent_and_updates_fields() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();
        let id1 = upsert_repo(&pool, &repo("idem/x", Some("Python")), date).await.unwrap();
        let id2 = upsert_repo(&pool, &repo("idem/x", Some("Go")), date).await.unwrap();
        assert_eq!(id1, id2);
        // Insert a snapshot so languages_with_counts (which JOINs snapshots) can see the repo
        upsert_snapshot(
            &pool,
            id2,
            date,
            Board::TopStars,
            &SnapshotInput {
                stars: 1,
                forks: 0,
                watchers: None,
                stars_today: None,
            },
        )
        .await
        .unwrap();
        let langs = languages_with_counts(&pool, date, Board::TopStars)
            .await
            .unwrap();
        assert!(langs.iter().any(|(lang, _)| lang == "Go"));
    }

    #[tokio::test]
    #[serial]
    async fn upsert_repo_writes_topics_languages_and_enriched_at() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();
        let mut r = repo("enrich/x", Some("Rust"));
        r.topics = vec!["ai".into(), "llm".into()];
        r.language_names = vec!["Rust".into(), "Python".into()];
        r.languages_json = serde_json::json!([
            {"name": "Rust", "pct": 80.0, "bytes": 800},
            {"name": "Python", "pct": 20.0, "bytes": 200}
        ]);
        let id = upsert_repo(&pool, &r, date).await.unwrap();
        let row = sqlx::query!(
            r#"SELECT topics AS "topics!", language_names AS "language_names!",
                      languages AS "languages!", last_enriched_at
               FROM repos WHERE id = $1"#,
            id
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.topics, vec!["ai".to_string(), "llm".to_string()]);
        assert_eq!(
            row.language_names,
            vec!["Rust".to_string(), "Python".to_string()]
        );
        assert!(row.languages.is_array());
        assert!(row.last_enriched_at.is_some());
    }

    #[tokio::test]
    #[serial]
    async fn upsert_repo_empty_enrichment_preserves_existing() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();
        let mut r = repo("enrich/preserve", Some("Rust"));
        r.topics = vec!["ai".into()];
        r.language_names = vec!["Rust".into()];
        r.languages_json = serde_json::json!([{"name": "Rust", "pct": 100.0, "bytes": 10}]);
        let id = upsert_repo(&pool, &r, date).await.unwrap();

        // Board re-upsert with empty enrichment must not wipe.
        let bare = repo("enrich/preserve", Some("Go"));
        upsert_repo(&pool, &bare, date).await.unwrap();
        let row = sqlx::query!(
            r#"SELECT language, topics AS "topics!", language_names AS "language_names!",
                      languages AS "languages!", last_enriched_at
               FROM repos WHERE id = $1"#,
            id
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.language.as_deref(), Some("Go"));
        assert_eq!(row.topics, vec!["ai".to_string()]);
        assert_eq!(row.language_names, vec!["Rust".to_string()]);
        assert!(row.languages.as_array().map(|a| !a.is_empty()).unwrap_or(false));
        assert!(row.last_enriched_at.is_some());
    }

    #[tokio::test]
    #[serial]
    async fn upsert_snapshot_same_day_same_board_one_row() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();
        let id = upsert_repo(&pool, &repo("snap/x", None), date).await.unwrap();
        let snap = SnapshotInput {
            stars: 10,
            forks: 1,
            watchers: None,
            stars_today: None,
        };
        upsert_snapshot(&pool, id, date, Board::TopStars, &snap)
            .await
            .unwrap();
        let snap2 = SnapshotInput {
            stars: 11,
            forks: 1,
            watchers: None,
            stars_today: None,
        };
        upsert_snapshot(&pool, id, date, Board::TopStars, &snap2)
            .await
            .unwrap();
        let rows = top_by_stars(&pool, date, LeaderboardFilter::empty(), 100)
            .await
            .unwrap();
        let snap_rows: Vec<_> = rows
            .into_iter()
            .filter(|r| r.full_name.starts_with("snap/"))
            .collect();
        assert_eq!(snap_rows.len(), 1);
        assert_eq!(snap_rows[0].stars, 11);
    }

    #[tokio::test]
    #[serial]
    async fn languages_count_matches_board_list() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();
        // Unique language so no other test's rows pollute the count.
        let lang = "ZigLangCount";
        // Repo A appears on 3 boards for the same date; repo B only on top_forks.
        let id_a = upsert_repo(&pool, &repo("lcnt/a", Some(lang)), date)
            .await
            .unwrap();
        for board in [Board::TopStars, Board::TopForks, Board::TrendingDaily] {
            upsert_snapshot(
                &pool,
                id_a,
                date,
                board,
                &SnapshotInput {
                    stars: 5,
                    forks: 5,
                    watchers: None,
                    stars_today: Some(5),
                },
            )
            .await
            .unwrap();
        }
        let id_b = upsert_repo(&pool, &repo("lcnt/b", Some(lang)), date)
            .await
            .unwrap();
        upsert_snapshot(
            &pool,
            id_b,
            date,
            Board::TopForks,
            &SnapshotInput {
                stars: 1,
                forks: 9,
                watchers: None,
                stars_today: None,
            },
        )
        .await
        .unwrap();

        // The language count must equal the number of rows the top_stars
        // leaderboard actually returns for that language: multi-board repos
        // count once, repos on other boards are excluded.
        let langs = languages_with_counts(&pool, date, Board::TopStars)
            .await
            .unwrap();
        let cnt = langs.iter().find(|(l, _)| l == lang).map(|(_, c)| *c);
        let rows = top_by_stars(&pool, date, lang_filter(Some(lang)), 100)
            .await
            .unwrap();
        assert_eq!(
            cnt,
            Some(rows.len() as i64),
            "language count must match the filtered board list size"
        );
    }

    #[tokio::test]
    #[serial]
    async fn rank_recomputed_after_language_filter() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();
        // Use a unique language so no other test's rows appear in the filtered query
        let unique_lang = "ZigTestRank";
        for (name, lang, stars) in [
            ("rank/py1", unique_lang, 300),
            ("rank/py2", unique_lang, 100),
            ("rank/rs1", "Rust", 200),
        ] {
            let id = upsert_repo(&pool, &repo(name, Some(lang)), date)
                .await
                .unwrap();
            upsert_snapshot(
                &pool,
                id,
                date,
                Board::TopStars,
                &SnapshotInput {
                    stars,
                    forks: 0,
                    watchers: None,
                    stars_today: None,
                },
            )
            .await
            .unwrap();
        }
        // UNFILTERED: only assert ordering and strictly-increasing ranks among our own rows
        let all = top_by_stars(&pool, date, LeaderboardFilter::empty(), 100)
            .await
            .unwrap();
        let mine: Vec<_> = all
            .into_iter()
            .filter(|r| r.full_name.starts_with("rank/"))
            .collect();
        assert_eq!(
            mine.iter().map(|r| r.full_name.clone()).collect::<Vec<_>>(),
            vec!["rank/py1", "rank/rs1", "rank/py2"]
        );
        assert!(
            mine.windows(2).all(|w| w[0].rank < w[1].rank),
            "ranks must be strictly increasing: {:?}",
            mine.iter().map(|r| r.rank).collect::<Vec<_>>()
        );
        // LANGUAGE-FILTERED: unique language guarantees we control all matching rows
        let filtered = top_by_stars(&pool, date, lang_filter(Some(unique_lang)), 100)
            .await
            .unwrap();
        assert_eq!(
            filtered
                .iter()
                .map(|r| r.full_name.clone())
                .collect::<Vec<_>>(),
            vec!["rank/py1", "rank/py2"]
        );
        assert_eq!(
            filtered.iter().map(|r| r.rank).collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[tokio::test]
    #[serial]
    async fn topics_and_filter_requires_all_topics() {
        let pool = test_pool().await;
        // Unique date so we fully control the result set.
        let date = NaiveDate::from_ymd_opt(2099, 2, 1).unwrap();
        sqlx::query(
            "DELETE FROM snapshots WHERE repo_id IN (SELECT id FROM repos WHERE full_name LIKE 'tand/%')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM repos WHERE full_name LIKE 'tand/%'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM snapshots WHERE snapshot_date = $1")
            .bind(date)
            .execute(&pool)
            .await
            .unwrap();

        for (name, topics, stars) in [
            ("tand/both", &["ai", "llm"][..], 300),
            ("tand/ai_only", &["ai"][..], 200),
            ("tand/llm_only", &["llm"][..], 100),
        ] {
            let id = upsert_repo(&pool, &repo_with_topics(name, Some("Rust"), topics), date)
                .await
                .unwrap();
            upsert_snapshot(
                &pool,
                id,
                date,
                Board::TopStars,
                &SnapshotInput {
                    stars,
                    forks: 0,
                    watchers: None,
                    stars_today: None,
                },
            )
            .await
            .unwrap();
        }

        let topics = vec!["ai".to_string(), "llm".to_string()];
        let filter = LeaderboardFilter {
            topics: Some(&topics),
            topic_mode: TopicMode::And,
            ..LeaderboardFilter::empty()
        };
        let rows = top_by_stars(&pool, date, filter, 100).await.unwrap();
        assert_eq!(
            rows.iter().map(|r| r.full_name.clone()).collect::<Vec<_>>(),
            vec!["tand/both"]
        );
        assert_eq!(rows[0].rank, 1);
        assert_eq!(rows[0].topics, vec!["ai".to_string(), "llm".to_string()]);
    }

    #[tokio::test]
    #[serial]
    async fn topics_or_filter_matches_any_topic() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2099, 2, 2).unwrap();
        sqlx::query(
            "DELETE FROM snapshots WHERE repo_id IN (SELECT id FROM repos WHERE full_name LIKE 'tor/%')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM repos WHERE full_name LIKE 'tor/%'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM snapshots WHERE snapshot_date = $1")
            .bind(date)
            .execute(&pool)
            .await
            .unwrap();

        for (name, topics, stars) in [
            ("tor/both", &["ai", "llm"][..], 300),
            ("tor/ai_only", &["ai"][..], 200),
            ("tor/other", &["rust"][..], 100),
        ] {
            let id = upsert_repo(&pool, &repo_with_topics(name, None, topics), date)
                .await
                .unwrap();
            upsert_snapshot(
                &pool,
                id,
                date,
                Board::TopStars,
                &SnapshotInput {
                    stars,
                    forks: 0,
                    watchers: None,
                    stars_today: None,
                },
            )
            .await
            .unwrap();
        }

        let topics = vec!["ai".to_string(), "llm".to_string()];
        let filter = LeaderboardFilter {
            topics: Some(&topics),
            topic_mode: TopicMode::Or,
            ..LeaderboardFilter::empty()
        };
        let rows = top_by_stars(&pool, date, filter, 100).await.unwrap();
        let names: Vec<_> = rows.iter().map(|r| r.full_name.clone()).collect();
        assert_eq!(names, vec!["tor/both", "tor/ai_only"]);
        assert_eq!(rows.iter().map(|r| r.rank).collect::<Vec<_>>(), vec![1, 2]);
    }

    #[tokio::test]
    #[serial]
    async fn languages_array_filter_or_semantics() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2099, 2, 3).unwrap();
        sqlx::query(
            "DELETE FROM snapshots WHERE repo_id IN (SELECT id FROM repos WHERE full_name LIKE 'langf/%')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM repos WHERE full_name LIKE 'langf/%'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM snapshots WHERE snapshot_date = $1")
            .bind(date)
            .execute(&pool)
            .await
            .unwrap();

        for (name, language_names, stars) in [
            ("langf/py_rs", vec!["Python", "Rust"], 300),
            ("langf/py", vec!["Python"], 200),
            ("langf/go", vec!["Go"], 100),
        ] {
            let mut r = repo(name, language_names.first().copied());
            r.language_names = language_names.into_iter().map(String::from).collect();
            let id = upsert_repo(&pool, &r, date).await.unwrap();
            upsert_snapshot(
                &pool,
                id,
                date,
                Board::TopStars,
                &SnapshotInput {
                    stars,
                    forks: 0,
                    watchers: None,
                    stars_today: None,
                },
            )
            .await
            .unwrap();
        }

        let langs = vec!["Rust".to_string(), "Go".to_string()];
        let filter = LeaderboardFilter {
            languages: Some(&langs),
            ..LeaderboardFilter::empty()
        };
        let rows = top_by_stars(&pool, date, filter, 100).await.unwrap();
        assert_eq!(
            rows.iter().map(|r| r.full_name.clone()).collect::<Vec<_>>(),
            vec!["langf/py_rs", "langf/go"]
        );
    }

    /// Multi-language filter must match primary `repos.language` even when
    /// `language_names` is empty (common before full languages enrichment).
    #[tokio::test]
    #[serial]
    async fn languages_filter_matches_primary_language_when_names_empty() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2099, 2, 14).unwrap();
        sqlx::query(
            "DELETE FROM snapshots WHERE repo_id IN (SELECT id FROM repos WHERE full_name LIKE 'primlang/%')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM repos WHERE full_name LIKE 'primlang/%'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM snapshots WHERE snapshot_date = $1")
            .bind(date)
            .execute(&pool)
            .await
            .unwrap();

        for (name, lang, stars) in [
            ("primlang/py", Some("Python"), 300),
            ("primlang/rs", Some("Rust"), 200),
            ("primlang/none", None, 100),
        ] {
            let r = repo(name, lang);
            assert!(r.language_names.is_empty());
            let id = upsert_repo(&pool, &r, date).await.unwrap();
            upsert_snapshot(
                &pool,
                id,
                date,
                Board::TopStars,
                &SnapshotInput {
                    stars,
                    forks: 0,
                    watchers: None,
                    stars_today: None,
                },
            )
            .await
            .unwrap();
        }

        let langs = vec!["Python".to_string()];
        let filter = LeaderboardFilter {
            languages: Some(&langs),
            ..LeaderboardFilter::empty()
        };
        let rows = top_by_stars(&pool, date, filter, 100).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].full_name, "primlang/py");
    }

    #[tokio::test]
    #[serial]
    async fn q_matches_full_name_description_or_topic() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2099, 2, 4).unwrap();
        sqlx::query(
            "DELETE FROM snapshots WHERE repo_id IN (SELECT id FROM repos WHERE full_name LIKE 'qfind/%')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM repos WHERE full_name LIKE 'qfind/%'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM snapshots WHERE snapshot_date = $1")
            .bind(date)
            .execute(&pool)
            .await
            .unwrap();

        // name match
        let id = upsert_repo(&pool, &repo("qfind/alpha-widget", None), date)
            .await
            .unwrap();
        upsert_snapshot(
            &pool,
            id,
            date,
            Board::TopStars,
            &SnapshotInput {
                stars: 30,
                forks: 0,
                watchers: None,
                stars_today: None,
            },
        )
        .await
        .unwrap();

        // description match
        let mut r = repo("qfind/beta", None);
        r.description = Some("awesome widget toolkit".into());
        let id = upsert_repo(&pool, &r, date).await.unwrap();
        upsert_snapshot(
            &pool,
            id,
            date,
            Board::TopStars,
            &SnapshotInput {
                stars: 20,
                forks: 0,
                watchers: None,
                stars_today: None,
            },
        )
        .await
        .unwrap();

        // topic match
        let id = upsert_repo(
            &pool,
            &repo_with_topics("qfind/gamma", None, &["widget-framework"]),
            date,
        )
        .await
        .unwrap();
        upsert_snapshot(
            &pool,
            id,
            date,
            Board::TopStars,
            &SnapshotInput {
                stars: 10,
                forks: 0,
                watchers: None,
                stars_today: None,
            },
        )
        .await
        .unwrap();

        // no match
        let id = upsert_repo(&pool, &repo("qfind/delta", None), date)
            .await
            .unwrap();
        upsert_snapshot(
            &pool,
            id,
            date,
            Board::TopStars,
            &SnapshotInput {
                stars: 5,
                forks: 0,
                watchers: None,
                stars_today: None,
            },
        )
        .await
        .unwrap();

        let filter = LeaderboardFilter {
            q: Some("widget"),
            ..LeaderboardFilter::empty()
        };
        let rows = top_by_stars(&pool, date, filter, 100).await.unwrap();
        let names: Vec<_> = rows.iter().map(|r| r.full_name.clone()).collect();
        assert_eq!(
            names,
            vec!["qfind/alpha-widget", "qfind/beta", "qfind/gamma"]
        );
    }

    #[tokio::test]
    #[serial]
    async fn trending_ordered_by_stars_today() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();
        for (name, today) in [("trend/t1", 5), ("trend/t2", 50)] {
            let id = upsert_repo(&pool, &repo(name, None), date).await.unwrap();
            upsert_snapshot(
                &pool,
                id,
                date,
                Board::TrendingDaily,
                &SnapshotInput {
                    stars: today,
                    forks: 0,
                    watchers: None,
                    stars_today: Some(today),
                },
            )
            .await
            .unwrap();
        }
        let rows = trending(&pool, date, LeaderboardFilter::empty(), 100)
            .await
            .unwrap();
        let rows: Vec<_> = rows
            .into_iter()
            .filter(|r| r.full_name.starts_with("trend/"))
            .collect();
        assert_eq!(rows[0].full_name, "trend/t2");
        assert_eq!(rows[0].stars_today, Some(50));
    }

    #[tokio::test]
    #[serial]
    async fn latest_snapshot_date_returns_max() {
        let pool = test_pool().await;
        let d1 = NaiveDate::from_ymd_opt(2026, 8, 5).unwrap();
        let d2 = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();
        // Clean up data from previous runs of THIS test only (scoped by repo prefix).
        // TopWatchers is used exclusively by this test, so this DELETE is safe.
        sqlx::query(
            "DELETE FROM snapshots WHERE repo_id IN (SELECT id FROM repos WHERE full_name LIKE 'late/%') AND board = 'top_watchers'",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM repos WHERE full_name LIKE 'late/%'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            latest_snapshot_date(&pool, Board::TopWatchers)
                .await
                .unwrap(),
            None
        );
        for d in [d1, d2] {
            let id = upsert_repo(&pool, &repo("late/x", None), d).await.unwrap();
            upsert_snapshot(
                &pool,
                id,
                d,
                Board::TopWatchers,
                &SnapshotInput {
                    stars: 1,
                    forks: 0,
                    watchers: None,
                    stars_today: None,
                },
            )
            .await
            .unwrap();
        }
        assert_eq!(
            latest_snapshot_date(&pool, Board::TopWatchers)
                .await
                .unwrap(),
            Some(d2)
        );
    }

    #[tokio::test]
    #[serial]
    async fn board_count_counts_rows_for_board_and_date() {
        let pool = test_pool().await;
        // Unique date so board_count is not polluted by other serial tests' rows.
        let date = NaiveDate::from_ymd_opt(2099, 1, 1).unwrap();
        sqlx::query(
            "DELETE FROM snapshots WHERE repo_id IN (SELECT id FROM repos WHERE full_name LIKE 'bcnt/%')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM repos WHERE full_name LIKE 'bcnt/%'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM snapshots WHERE snapshot_date = $1")
            .bind(date)
            .execute(&pool)
            .await
            .unwrap();

        for (name, stars) in [("bcnt/a", 10), ("bcnt/b", 20)] {
            let id = upsert_repo(&pool, &repo(name, Some("Rust")), date)
                .await
                .unwrap();
            upsert_snapshot(
                &pool,
                id,
                date,
                Board::TopStars,
                &SnapshotInput {
                    stars,
                    forks: 0,
                    watchers: None,
                    stars_today: None,
                },
            )
            .await
            .unwrap();
        }
        // Another board must not count toward TopStars
        let id = upsert_repo(&pool, &repo("bcnt/c", None), date).await.unwrap();
        upsert_snapshot(
            &pool,
            id,
            date,
            Board::TopForks,
            &SnapshotInput {
                stars: 1,
                forks: 9,
                watchers: None,
                stars_today: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(
            board_count(&pool, date, Board::TopStars).await.unwrap(),
            2
        );
        assert_eq!(
            board_count(&pool, date, Board::TopForks).await.unwrap(),
            1
        );
        let rows = top_by_stars(&pool, date, LeaderboardFilter::empty(), 1000)
            .await
            .unwrap();
        let mine = rows
            .iter()
            .filter(|r| r.full_name.starts_with("bcnt/"))
            .count();
        assert_eq!(mine, 2);
    }

    #[tokio::test]
    #[serial]
    async fn cleanup_expired_refresh_tokens_only_deletes_expired() {
        let pool = test_pool().await;
        sqlx::query("TRUNCATE users, invite_codes, refresh_tokens")
            .execute(&pool)
            .await
            .unwrap();

        let uid: i64 = sqlx::query_scalar(
            "INSERT INTO users (username, password_hash) VALUES ('cleanup_u', 'h') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO refresh_tokens (user_id, token_hash, expires_at) VALUES ($1, 'h_old', now() - interval '1 day')",
        )
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO refresh_tokens (user_id, token_hash, expires_at) VALUES ($1, 'h_new', now() + interval '1 day')",
        )
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();

        let deleted = cleanup_expired_refresh_tokens(&pool).await.unwrap();
        assert_eq!(deleted, 1);
        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM refresh_tokens")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(left, 1);
    }

    #[test]
    fn board_tracked_daily_roundtrip() {
        assert_eq!(Board::TrackedDaily.as_str(), "tracked_daily");
        assert_eq!(Board::parse("tracked_daily"), Some(Board::TrackedDaily));
    }

    #[test]
    fn tracked_repo_limit_is_fifty() {
        assert_eq!(TRACKED_REPO_LIMIT, 50);
    }

    async fn insert_test_user(pool: &PgPool, username: &str) -> i64 {
        let existing: Option<i64> =
            sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
                .bind(username)
                .fetch_optional(pool)
                .await
                .unwrap();
        let uid = if let Some(id) = existing {
            id
        } else {
            sqlx::query_scalar(
                "INSERT INTO users (username, password_hash) VALUES ($1, 'h') RETURNING id",
            )
            .bind(username)
            .fetch_one(pool)
            .await
            .unwrap()
        };
        // Isolate track rows for this user between re-runs.
        sqlx::query("DELETE FROM user_tracked_repos WHERE user_id = $1")
            .bind(uid)
            .execute(pool)
            .await
            .unwrap();
        uid
    }

    #[tokio::test]
    #[serial]
    async fn track_untrack_is_tracked_and_count() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2026, 8, 7).unwrap();
        let uid = insert_test_user(&pool, "track_u1").await;
        let id_a = upsert_repo(&pool, &repo("tracktest/a", Some("Rust")), date)
            .await
            .unwrap();
        let id_b = upsert_repo(&pool, &repo("tracktest/b", Some("Go")), date)
            .await
            .unwrap();

        assert_eq!(count_tracked(&pool, uid).await.unwrap(), 0);
        assert!(!is_tracked(&pool, uid, id_a).await.unwrap());

        track_repo(&pool, uid, id_a).await.unwrap();
        track_repo(&pool, uid, id_b).await.unwrap();
        // idempotent
        track_repo(&pool, uid, id_a).await.unwrap();

        assert_eq!(count_tracked(&pool, uid).await.unwrap(), 2);
        assert!(is_tracked(&pool, uid, id_a).await.unwrap());
        assert!(is_tracked(&pool, uid, id_b).await.unwrap());

        let removed = untrack_repo(&pool, uid, "tracktest/a").await.unwrap();
        assert!(removed);
        assert!(!is_tracked(&pool, uid, id_a).await.unwrap());
        assert_eq!(count_tracked(&pool, uid).await.unwrap(), 1);

        let again = untrack_repo(&pool, uid, "tracktest/a").await.unwrap();
        assert!(!again);
        let missing = untrack_repo(&pool, uid, "tracktest/nope").await.unwrap();
        assert!(!missing);
    }

    #[tokio::test]
    #[serial]
    async fn list_tracked_filters_and_metrics_prefer_tracked_daily() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2026, 8, 7).unwrap();
        let uid = insert_test_user(&pool, "track_u2").await;

        let mut r_ai = repo_with_topics("tracklist/ai", Some("Python"), &["ai", "ml"]);
        r_ai.language_names = vec!["Python".into(), "Rust".into()];
        r_ai.languages_json = serde_json::json!([
            {"name": "Python", "pct": 70.0},
            {"name": "Rust", "pct": 30.0}
        ]);
        let id_ai = upsert_repo(&pool, &r_ai, date).await.unwrap();

        let mut r_web = repo_with_topics("tracklist/web", Some("TypeScript"), &["web"]);
        r_web.language_names = vec!["TypeScript".into()];
        let id_web = upsert_repo(&pool, &r_web, date).await.unwrap();

        track_repo(&pool, uid, id_ai).await.unwrap();
        track_repo(&pool, uid, id_web).await.unwrap();

        // Public board metrics should lose to tracked_daily when both exist.
        upsert_snapshot(
            &pool,
            id_ai,
            date,
            Board::TopStars,
            &SnapshotInput {
                stars: 100,
                forks: 10,
                watchers: Some(5),
                stars_today: None,
            },
        )
        .await
        .unwrap();
        upsert_snapshot(
            &pool,
            id_ai,
            date,
            Board::TrackedDaily,
            &SnapshotInput {
                stars: 999,
                forks: 88,
                watchers: Some(7),
                stars_today: Some(3),
            },
        )
        .await
        .unwrap();

        let all = list_tracked(&pool, uid, LeaderboardFilter::empty())
            .await
            .unwrap();
        assert_eq!(all.len(), 2);
        let ai = all.iter().find(|r| r.full_name == "tracklist/ai").unwrap();
        assert_eq!(ai.stars, Some(999));
        assert_eq!(ai.forks, Some(88));
        assert_eq!(ai.watchers, Some(7));
        assert_eq!(ai.stars_today, Some(3));
        assert!(ai.topics.contains(&"ai".to_string()));

        let topics = vec!["ai".to_string()];
        let by_topic = list_tracked(
            &pool,
            uid,
            LeaderboardFilter {
                topics: Some(&topics),
                topic_mode: TopicMode::And,
                ..LeaderboardFilter::empty()
            },
        )
        .await
        .unwrap();
        assert_eq!(by_topic.len(), 1);
        assert_eq!(by_topic[0].full_name, "tracklist/ai");

        let langs = vec!["TypeScript".to_string()];
        let by_lang = list_tracked(
            &pool,
            uid,
            LeaderboardFilter {
                languages: Some(&langs),
                ..LeaderboardFilter::empty()
            },
        )
        .await
        .unwrap();
        assert_eq!(by_lang.len(), 1);
        assert_eq!(by_lang[0].full_name, "tracklist/web");

        let by_q = list_tracked(
            &pool,
            uid,
            LeaderboardFilter {
                q: Some("tracklist/ai"),
                ..LeaderboardFilter::empty()
            },
        )
        .await
        .unwrap();
        assert_eq!(by_q.len(), 1);
        assert_eq!(by_q[0].full_name, "tracklist/ai");
    }

    #[tokio::test]
    #[serial]
    async fn list_all_tracked_full_names_is_distinct() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2026, 8, 7).unwrap();
        let u1 = insert_test_user(&pool, "track_u3a").await;
        let u2 = insert_test_user(&pool, "track_u3b").await;
        let id = upsert_repo(&pool, &repo("trackall/shared", None), date)
            .await
            .unwrap();
        let id2 = upsert_repo(&pool, &repo("trackall/only1", None), date)
            .await
            .unwrap();
        track_repo(&pool, u1, id).await.unwrap();
        track_repo(&pool, u2, id).await.unwrap(); // same repo, two users
        track_repo(&pool, u1, id2).await.unwrap();

        let names = list_all_tracked_full_names(&pool).await.unwrap();
        let mine: Vec<_> = names
            .into_iter()
            .filter(|n| n.starts_with("trackall/"))
            .collect();
        assert_eq!(mine, vec!["trackall/only1".to_string(), "trackall/shared".to_string()]);
    }

    #[tokio::test]
    #[serial]
    async fn repos_exist_full_names_batch() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2026, 8, 7).unwrap();
        upsert_repo(&pool, &repo("existbatch/a", None), date)
            .await
            .unwrap();
        upsert_repo(&pool, &repo("existbatch/b", None), date)
            .await
            .unwrap();

        let empty = repos_exist_full_names(&pool, &[]).await.unwrap();
        assert!(empty.is_empty());

        let names = vec![
            "existbatch/a".to_string(),
            "existbatch/missing".to_string(),
            "existbatch/b".to_string(),
        ];
        let found = repos_exist_full_names(&pool, &names).await.unwrap();
        assert_eq!(found.len(), 2);
        assert!(found.contains("existbatch/a"));
        assert!(found.contains("existbatch/b"));
        assert!(!found.contains("existbatch/missing"));
    }

    #[tokio::test]
    #[serial]
    async fn count_tracked_respects_per_user_cap_boundary() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2026, 8, 7).unwrap();
        let uid = insert_test_user(&pool, "track_u_cap").await;

        // Seed TRACKED_REPO_LIMIT distinct repos and track them all.
        for i in 0..TRACKED_REPO_LIMIT {
            let name = format!("trackcap/repo{i}");
            let rid = upsert_repo(&pool, &repo(&name, None), date).await.unwrap();
            track_repo(&pool, uid, rid).await.unwrap();
        }
        assert_eq!(
            count_tracked(&pool, uid).await.unwrap(),
            TRACKED_REPO_LIMIT
        );
        // Cap is enforced by API using this count; store still allows insert
        // beyond limit (caller decides). Documented contract:
        assert!(count_tracked(&pool, uid).await.unwrap() >= TRACKED_REPO_LIMIT);
    }

    #[tokio::test]
    #[serial]
    async fn exclude_archived_and_active_within_days_filters() {
        use chrono::{Duration, Utc};

        let pool = test_pool().await;
        // Unique date so we fully control the result set.
        let date = NaiveDate::from_ymd_opt(2099, 3, 1).unwrap();
        sqlx::query(
            "DELETE FROM snapshots WHERE repo_id IN (SELECT id FROM repos WHERE full_name LIKE 'healthf/%')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM repos WHERE full_name LIKE 'healthf/%'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM snapshots WHERE snapshot_date = $1")
            .bind(date)
            .execute(&pool)
            .await
            .unwrap();

        let now = Utc::now();
        let id_active = upsert_repo(&pool, &repo("healthf/active", Some("Rust")), date)
            .await
            .unwrap();
        update_repo_health(
            &pool,
            id_active,
            Some(now - Duration::days(10)),
            false,
            Some(3),
            Some(now - Duration::days(400)),
            Some(now - Duration::days(5)),
        )
        .await
        .unwrap();

        let id_archived = upsert_repo(&pool, &repo("healthf/archived", Some("Rust")), date)
            .await
            .unwrap();
        update_repo_health(
            &pool,
            id_archived,
            Some(now - Duration::days(1)),
            true,
            Some(0),
            Some(now - Duration::days(500)),
            None,
        )
        .await
        .unwrap();

        let id_stale = upsert_repo(&pool, &repo("healthf/stale", Some("Rust")), date)
            .await
            .unwrap();
        update_repo_health(
            &pool,
            id_stale,
            Some(now - Duration::days(120)),
            false,
            Some(10),
            Some(now - Duration::days(800)),
            None,
        )
        .await
        .unwrap();

        for (id, stars) in [(id_active, 300), (id_archived, 200), (id_stale, 100)] {
            upsert_snapshot(
                &pool,
                id,
                date,
                Board::TopStars,
                &SnapshotInput {
                    stars,
                    forks: 0,
                    watchers: None,
                    stars_today: None,
                },
            )
            .await
            .unwrap();
        }

        // No health filter: all three.
        let all = top_by_stars(&pool, date, LeaderboardFilter::empty(), 100)
            .await
            .unwrap();
        let names: Vec<_> = all.iter().map(|r| r.full_name.clone()).collect();
        assert_eq!(
            names,
            vec![
                "healthf/active".to_string(),
                "healthf/archived".to_string(),
                "healthf/stale".to_string()
            ]
        );
        assert!(!all[0].archived);
        assert!(all[1].archived);
        assert_eq!(all[0].open_issues_count, Some(3));
        assert!(all[0].pushed_at.is_some());
        assert!(all[0].created_at_gh.is_some());
        assert!(all[0].latest_release_at.is_some());

        // exclude_archived: drops archived only.
        let excl = top_by_stars(
            &pool,
            date,
            LeaderboardFilter {
                exclude_archived: true,
                ..LeaderboardFilter::empty()
            },
            100,
        )
        .await
        .unwrap();
        assert_eq!(
            excl.iter().map(|r| r.full_name.clone()).collect::<Vec<_>>(),
            vec!["healthf/active".to_string(), "healthf/stale".to_string()]
        );

        // active_within_days=90: only recent non-archived push (also implies not archived).
        let active = top_by_stars(
            &pool,
            date,
            LeaderboardFilter {
                active_within_days: Some(90),
                ..LeaderboardFilter::empty()
            },
            100,
        )
        .await
        .unwrap();
        assert_eq!(
            active
                .iter()
                .map(|r| r.full_name.clone())
                .collect::<Vec<_>>(),
            vec!["healthf/active".to_string()]
        );

        // Both filters: same as active_within alone for this fixture.
        let both = top_by_stars(
            &pool,
            date,
            LeaderboardFilter {
                exclude_archived: true,
                active_within_days: Some(90),
                ..LeaderboardFilter::empty()
            },
            100,
        )
        .await
        .unwrap();
        assert_eq!(both.len(), 1);
        assert_eq!(both[0].full_name, "healthf/active");
    }

    #[tokio::test]
    #[serial]
    async fn update_repo_health_write_and_readback() {
        use chrono::{Duration, Utc};

        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2099, 3, 2).unwrap();
        let id = upsert_repo(&pool, &repo("healthw/x", Some("Go")), date)
            .await
            .unwrap();

        // Defaults from migration / insert.
        let before = sqlx::query!(
            r#"SELECT pushed_at, archived AS "archived!", open_issues_count,
                      created_at_gh, latest_release_at
               FROM repos WHERE id = $1"#,
            id
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(before.pushed_at.is_none());
        assert!(!before.archived);
        assert!(before.open_issues_count.is_none());

        let now = Utc::now();
        let pushed = now - Duration::days(2);
        let created = now - Duration::days(1000);
        let release = now - Duration::days(7);
        update_repo_health(
            &pool,
            id,
            Some(pushed),
            true,
            Some(42),
            Some(created),
            Some(release),
        )
        .await
        .unwrap();

        let after = sqlx::query!(
            r#"SELECT pushed_at, archived AS "archived!", open_issues_count,
                      created_at_gh, latest_release_at
               FROM repos WHERE id = $1"#,
            id
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(after.archived);
        assert_eq!(after.open_issues_count, Some(42));
        assert!(after.pushed_at.is_some());
        assert!(after.created_at_gh.is_some());
        assert!(after.latest_release_at.is_some());

        // Board upsert without health must not wipe (upsert_repo leaves health alone).
        upsert_repo(&pool, &repo("healthw/x", Some("Rust")), date)
            .await
            .unwrap();
        let preserved = sqlx::query!(
            r#"SELECT language, archived AS "archived!", open_issues_count
               FROM repos WHERE id = $1"#,
            id
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(preserved.language.as_deref(), Some("Rust"));
        assert!(preserved.archived);
        assert_eq!(preserved.open_issues_count, Some(42));
    }

    #[tokio::test]
    #[serial]
    async fn update_repo_health_keep_release_preserves_latest() {
        use chrono::{Duration, Utc};

        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2099, 3, 4).unwrap();
        let id = upsert_repo(&pool, &repo("healthk/x", Some("Go")), date)
            .await
            .unwrap();

        let now = Utc::now();
        let release = now - Duration::days(7);
        update_repo_health(
            &pool,
            id,
            Some(now - Duration::days(30)),
            false,
            Some(1),
            Some(now - Duration::days(500)),
            Some(release),
        )
        .await
        .unwrap();

        // Transient release failure path: other fields refresh, latest_release_at stays.
        update_repo_health_keep_release(
            &pool,
            id,
            Some(now - Duration::days(1)),
            true,
            Some(99),
            Some(now - Duration::days(400)),
        )
        .await
        .unwrap();

        let row = sqlx::query!(
            r#"SELECT pushed_at, archived AS "archived!", open_issues_count,
                      created_at_gh, latest_release_at
               FROM repos WHERE id = $1"#,
            id
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(row.archived);
        assert_eq!(row.open_issues_count, Some(99));
        assert!(row.pushed_at.is_some());
        assert!(row.created_at_gh.is_some());
        let kept = row.latest_release_at.expect("latest_release_at preserved");
        assert_eq!(kept.timestamp(), release.timestamp());
    }

    #[tokio::test]
    #[serial]
    async fn list_tracked_exclude_archived_filter() {
        use chrono::{Duration, Utc};

        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2099, 3, 3).unwrap();
        let uid = insert_test_user(&pool, "track_health").await;

        let id_ok = upsert_repo(&pool, &repo("trackhealth/ok", None), date)
            .await
            .unwrap();
        let id_arc = upsert_repo(&pool, &repo("trackhealth/arc", None), date)
            .await
            .unwrap();
        update_repo_health(
            &pool,
            id_ok,
            Some(Utc::now() - Duration::days(5)),
            false,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        update_repo_health(
            &pool,
            id_arc,
            Some(Utc::now() - Duration::days(5)),
            true,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        track_repo(&pool, uid, id_ok).await.unwrap();
        track_repo(&pool, uid, id_arc).await.unwrap();

        let all = list_tracked(&pool, uid, LeaderboardFilter::empty())
            .await
            .unwrap();
        assert_eq!(all.len(), 2);

        let filtered = list_tracked(
            &pool,
            uid,
            LeaderboardFilter {
                exclude_archived: true,
                ..LeaderboardFilter::empty()
            },
        )
        .await
        .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].full_name, "trackhealth/ok");
        assert!(!filtered[0].archived);
        assert!(filtered[0].pushed_at.is_some());
    }
}
