use chrono::NaiveDate;
use sqlx::PgPool;

use crate::models::{Board, LeaderboardFilter, LeaderboardRow, RepoInput, SnapshotInput};

/// Bind helpers: empty slices / blank q mean "no filter" (SQL NULL).
fn filter_langs(f: &LeaderboardFilter<'_>) -> Option<Vec<String>> {
    f.languages
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
    sqlx::query_scalar!(
        r#"INSERT INTO repos (
               full_name, owner, name, html_url, language, description, first_seen,
               topics, languages, language_names, last_enriched_at
           )
           VALUES (
               $1, $2, $3, $4, $5, $6, $7,
               $8, $9, $10,
               CASE
                 WHEN cardinality($8::text[]) > 0 OR cardinality($10::text[]) > 0
                 THEN now()
                 ELSE NULL
               END
           )
           ON CONFLICT (full_name) DO UPDATE
           SET html_url = EXCLUDED.html_url,
               language = EXCLUDED.language,
               description = EXCLUDED.description,
               topics = EXCLUDED.topics,
               languages = EXCLUDED.languages,
               language_names = EXCLUDED.language_names,
               last_enriched_at = COALESCE(EXCLUDED.last_enriched_at, repos.last_enriched_at)
           RETURNING id"#,
        r.full_name,
        r.owner,
        r.name,
        r.html_url,
        r.language,
        r.description,
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

pub async fn top_by_stars(
    pool: &PgPool,
    date: NaiveDate,
    filter: LeaderboardFilter<'_>,
    limit: i64,
) -> Result<Vec<LeaderboardRow>, sqlx::Error> {
    let langs = filter_langs(&filter);
    let topics = filter_topics(&filter);
    let q = filter_q(&filter);
    sqlx::query_as!(
        LeaderboardRow,
        r#"SELECT ROW_NUMBER() OVER (ORDER BY s.stars DESC) AS "rank!",
                  r.full_name AS full_name, r.html_url AS html_url,
                  r.description AS description, r.language AS language,
                  r.topics AS "topics!", r.languages AS "languages!",
                  s.stars AS stars, s.forks AS forks, s.watchers AS watchers, s.stars_today AS stars_today
           FROM snapshots s JOIN repos r ON r.id = s.repo_id
           WHERE s.snapshot_date = $1 AND s.board = 'top_stars'
             AND ($2::text IS NULL OR r.language = $2)
             AND ($3::text[] IS NULL OR r.language_names && $3)
             AND (
               $4::text[] IS NULL
               OR ($5 = 'and' AND r.topics @> $4)
               OR ($5 = 'or' AND r.topics && $4)
             )
             AND (
               $6::text IS NULL
               OR r.full_name ILIKE '%' || $6 || '%'
               OR COALESCE(r.description, '') ILIKE '%' || $6 || '%'
               OR EXISTS (
                 SELECT 1 FROM unnest(r.topics) AS t(topic)
                 WHERE t.topic ILIKE '%' || $6 || '%'
               )
               OR EXISTS (
                 SELECT 1 FROM unnest(r.language_names) AS ln(name)
                 WHERE ln.name ILIKE '%' || $6 || '%'
               )
             )
           ORDER BY s.stars DESC
           LIMIT $7"#,
        date,
        filter.language,
        langs.as_deref(),
        topics.as_deref(),
        filter.topic_mode.as_str(),
        q,
        limit
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
    let topics = filter_topics(&filter);
    let q = filter_q(&filter);
    sqlx::query_as!(
        LeaderboardRow,
        r#"SELECT ROW_NUMBER() OVER (ORDER BY s.forks DESC) AS "rank!",
                  r.full_name AS full_name, r.html_url AS html_url,
                  r.description AS description, r.language AS language,
                  r.topics AS "topics!", r.languages AS "languages!",
                  s.stars AS stars, s.forks AS forks, s.watchers AS watchers, s.stars_today AS stars_today
           FROM snapshots s JOIN repos r ON r.id = s.repo_id
           WHERE s.snapshot_date = $1 AND s.board = 'top_forks'
             AND ($2::text IS NULL OR r.language = $2)
             AND ($3::text[] IS NULL OR r.language_names && $3)
             AND (
               $4::text[] IS NULL
               OR ($5 = 'and' AND r.topics @> $4)
               OR ($5 = 'or' AND r.topics && $4)
             )
             AND (
               $6::text IS NULL
               OR r.full_name ILIKE '%' || $6 || '%'
               OR COALESCE(r.description, '') ILIKE '%' || $6 || '%'
               OR EXISTS (
                 SELECT 1 FROM unnest(r.topics) AS t(topic)
                 WHERE t.topic ILIKE '%' || $6 || '%'
               )
               OR EXISTS (
                 SELECT 1 FROM unnest(r.language_names) AS ln(name)
                 WHERE ln.name ILIKE '%' || $6 || '%'
               )
             )
           ORDER BY s.forks DESC
           LIMIT $7"#,
        date,
        filter.language,
        langs.as_deref(),
        topics.as_deref(),
        filter.topic_mode.as_str(),
        q,
        limit
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
    let topics = filter_topics(&filter);
    let q = filter_q(&filter);
    sqlx::query_as!(
        LeaderboardRow,
        r#"SELECT ROW_NUMBER() OVER (ORDER BY s.watchers DESC NULLS LAST) AS "rank!",
                  r.full_name AS full_name, r.html_url AS html_url,
                  r.description AS description, r.language AS language,
                  r.topics AS "topics!", r.languages AS "languages!",
                  s.stars AS stars, s.forks AS forks, s.watchers AS watchers, s.stars_today AS stars_today
           FROM snapshots s JOIN repos r ON r.id = s.repo_id
           WHERE s.snapshot_date = $1 AND s.board = 'top_watchers'
             AND ($2::text IS NULL OR r.language = $2)
             AND ($3::text[] IS NULL OR r.language_names && $3)
             AND (
               $4::text[] IS NULL
               OR ($5 = 'and' AND r.topics @> $4)
               OR ($5 = 'or' AND r.topics && $4)
             )
             AND (
               $6::text IS NULL
               OR r.full_name ILIKE '%' || $6 || '%'
               OR COALESCE(r.description, '') ILIKE '%' || $6 || '%'
               OR EXISTS (
                 SELECT 1 FROM unnest(r.topics) AS t(topic)
                 WHERE t.topic ILIKE '%' || $6 || '%'
               )
               OR EXISTS (
                 SELECT 1 FROM unnest(r.language_names) AS ln(name)
                 WHERE ln.name ILIKE '%' || $6 || '%'
               )
             )
           ORDER BY s.watchers DESC NULLS LAST
           LIMIT $7"#,
        date,
        filter.language,
        langs.as_deref(),
        topics.as_deref(),
        filter.topic_mode.as_str(),
        q,
        limit
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
    let topics = filter_topics(&filter);
    let q = filter_q(&filter);
    sqlx::query_as!(
        LeaderboardRow,
        r#"SELECT ROW_NUMBER() OVER (ORDER BY s.stars_today DESC NULLS LAST) AS "rank!",
                  r.full_name AS full_name, r.html_url AS html_url,
                  r.description AS description, r.language AS language,
                  r.topics AS "topics!", r.languages AS "languages!",
                  s.stars AS stars, s.forks AS forks, s.watchers AS watchers, s.stars_today AS stars_today
           FROM snapshots s JOIN repos r ON r.id = s.repo_id
           WHERE s.snapshot_date = $1 AND s.board = 'trending_daily'
             AND ($2::text IS NULL OR r.language = $2)
             AND ($3::text[] IS NULL OR r.language_names && $3)
             AND (
               $4::text[] IS NULL
               OR ($5 = 'and' AND r.topics @> $4)
               OR ($5 = 'or' AND r.topics && $4)
             )
             AND (
               $6::text IS NULL
               OR r.full_name ILIKE '%' || $6 || '%'
               OR COALESCE(r.description, '') ILIKE '%' || $6 || '%'
               OR EXISTS (
                 SELECT 1 FROM unnest(r.topics) AS t(topic)
                 WHERE t.topic ILIKE '%' || $6 || '%'
               )
               OR EXISTS (
                 SELECT 1 FROM unnest(r.language_names) AS ln(name)
                 WHERE ln.name ILIKE '%' || $6 || '%'
               )
             )
           ORDER BY s.stars_today DESC NULLS LAST
           LIMIT $7"#,
        date,
        filter.language,
        langs.as_deref(),
        topics.as_deref(),
        filter.topic_mode.as_str(),
        q,
        limit
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
            topics: vec![],
            languages_json: RepoInput::languages_empty(),
            language_names: vec![],
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
}
