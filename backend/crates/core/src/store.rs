use chrono::NaiveDate;
use sqlx::PgPool;

use crate::models::{Board, LeaderboardRow, RepoInput, SnapshotInput};

pub async fn upsert_repo(pool: &PgPool, r: &RepoInput, today: NaiveDate) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"INSERT INTO repos (full_name, owner, name, html_url, language, description, first_seen)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           ON CONFLICT (full_name) DO UPDATE
           SET html_url = EXCLUDED.html_url,
               language = EXCLUDED.language,
               description = EXCLUDED.description
           RETURNING id"#,
        r.full_name,
        r.owner,
        r.name,
        r.html_url,
        r.language,
        r.description,
        today
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
    language: Option<&str>,
    limit: i64,
) -> Result<Vec<LeaderboardRow>, sqlx::Error> {
    sqlx::query_as!(
        LeaderboardRow,
        r#"SELECT ROW_NUMBER() OVER (ORDER BY s.stars DESC) AS "rank!",
                  r.full_name AS full_name, r.html_url AS html_url,
                  r.description AS description, r.language AS language,
                  s.stars AS stars, s.forks AS forks, s.watchers AS watchers, s.stars_today AS stars_today
           FROM snapshots s JOIN repos r ON r.id = s.repo_id
           WHERE s.snapshot_date = $1 AND s.board = 'top_stars'
             AND ($2::text IS NULL OR r.language = $2)
           ORDER BY s.stars DESC
           LIMIT $3"#,
        date,
        language,
        limit
    )
    .fetch_all(pool)
    .await
}

pub async fn top_by_forks(
    pool: &PgPool,
    date: NaiveDate,
    language: Option<&str>,
    limit: i64,
) -> Result<Vec<LeaderboardRow>, sqlx::Error> {
    sqlx::query_as!(
        LeaderboardRow,
        r#"SELECT ROW_NUMBER() OVER (ORDER BY s.forks DESC) AS "rank!",
                  r.full_name AS full_name, r.html_url AS html_url,
                  r.description AS description, r.language AS language,
                  s.stars AS stars, s.forks AS forks, s.watchers AS watchers, s.stars_today AS stars_today
           FROM snapshots s JOIN repos r ON r.id = s.repo_id
           WHERE s.snapshot_date = $1 AND s.board = 'top_forks'
             AND ($2::text IS NULL OR r.language = $2)
           ORDER BY s.forks DESC
           LIMIT $3"#,
        date,
        language,
        limit
    )
    .fetch_all(pool)
    .await
}

pub async fn top_by_watchers(
    pool: &PgPool,
    date: NaiveDate,
    language: Option<&str>,
    limit: i64,
) -> Result<Vec<LeaderboardRow>, sqlx::Error> {
    sqlx::query_as!(
        LeaderboardRow,
        r#"SELECT ROW_NUMBER() OVER (ORDER BY s.watchers DESC NULLS LAST) AS "rank!",
                  r.full_name AS full_name, r.html_url AS html_url,
                  r.description AS description, r.language AS language,
                  s.stars AS stars, s.forks AS forks, s.watchers AS watchers, s.stars_today AS stars_today
           FROM snapshots s JOIN repos r ON r.id = s.repo_id
           WHERE s.snapshot_date = $1 AND s.board = 'top_watchers'
             AND ($2::text IS NULL OR r.language = $2)
           ORDER BY s.watchers DESC NULLS LAST
           LIMIT $3"#,
        date,
        language,
        limit
    )
    .fetch_all(pool)
    .await
}

pub async fn trending(
    pool: &PgPool,
    date: NaiveDate,
    language: Option<&str>,
    limit: i64,
) -> Result<Vec<LeaderboardRow>, sqlx::Error> {
    sqlx::query_as!(
        LeaderboardRow,
        r#"SELECT ROW_NUMBER() OVER (ORDER BY s.stars_today DESC NULLS LAST) AS "rank!",
                  r.full_name AS full_name, r.html_url AS html_url,
                  r.description AS description, r.language AS language,
                  s.stars AS stars, s.forks AS forks, s.watchers AS watchers, s.stars_today AS stars_today
           FROM snapshots s JOIN repos r ON r.id = s.repo_id
           WHERE s.snapshot_date = $1 AND s.board = 'trending_daily'
             AND ($2::text IS NULL OR r.language = $2)
           ORDER BY s.stars_today DESC NULLS LAST
           LIMIT $3"#,
        date,
        language,
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

pub async fn languages_with_counts(pool: &PgPool, date: NaiveDate) -> Result<Vec<(String, i64)>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT r.language AS lang, COUNT(*) AS cnt
           FROM snapshots s JOIN repos r ON r.id = s.repo_id
           WHERE s.snapshot_date = $1 AND r.language IS NOT NULL
           GROUP BY r.language
           ORDER BY cnt DESC"#,
        date
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::models::{Board, RepoInput, SnapshotInput};
    use chrono::NaiveDate;
    use sqlx::PgPool;

    pub async fn test_pool() -> PgPool {
        let url = std::env::var("DATABASE_URL_TEST")
            .unwrap_or_else(|_| "postgres://ght:ght@localhost:5433/ghtrending_test".into());
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
        }
    }

    #[tokio::test]
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
            &SnapshotInput { stars: 1, forks: 0, watchers: None, stars_today: None },
        )
        .await
        .unwrap();
        let langs = languages_with_counts(&pool, date).await.unwrap();
        assert!(langs.iter().any(|(lang, _)| lang == "Go"));
    }

    #[tokio::test]
    async fn upsert_snapshot_same_day_same_board_one_row() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();
        let id = upsert_repo(&pool, &repo("snap/x", None), date).await.unwrap();
        let snap = SnapshotInput { stars: 10, forks: 1, watchers: None, stars_today: None };
        upsert_snapshot(&pool, id, date, Board::TopStars, &snap).await.unwrap();
        let snap2 = SnapshotInput { stars: 11, forks: 1, watchers: None, stars_today: None };
        upsert_snapshot(&pool, id, date, Board::TopStars, &snap2).await.unwrap();
        let rows = top_by_stars(&pool, date, None, 100).await.unwrap();
        let snap_rows: Vec<_> = rows.into_iter().filter(|r| r.full_name.starts_with("snap/")).collect();
        assert_eq!(snap_rows.len(), 1);
        assert_eq!(snap_rows[0].stars, 11);
    }

    #[tokio::test]
    async fn rank_recomputed_after_language_filter() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();
        for (name, lang, stars) in [("rank/py1", "Python", 300), ("rank/py2", "Python", 100), ("rank/rs1", "Rust", 200)] {
            let id = upsert_repo(&pool, &repo(name, Some(lang)), date).await.unwrap();
            upsert_snapshot(
                &pool,
                id,
                date,
                Board::TopStars,
                &SnapshotInput { stars, forks: 0, watchers: None, stars_today: None },
            )
            .await
            .unwrap();
        }
        let all = top_by_stars(&pool, date, None, 100).await.unwrap();
        let all: Vec<_> = all.into_iter().filter(|r| r.full_name.starts_with("rank/")).collect();
        assert_eq!(
            all.iter().map(|r| r.full_name.clone()).collect::<Vec<_>>(),
            vec!["rank/py1", "rank/rs1", "rank/py2"]
        );
        assert_eq!(all.iter().map(|r| r.rank).collect::<Vec<_>>(), vec![1, 2, 3]);
        let py = top_by_stars(&pool, date, Some("Python"), 100).await.unwrap();
        let py: Vec<_> = py.into_iter().filter(|r| r.full_name.starts_with("rank/")).collect();
        assert_eq!(
            py.iter().map(|r| r.full_name.clone()).collect::<Vec<_>>(),
            vec!["rank/py1", "rank/py2"]
        );
        assert_eq!(py.iter().map(|r| r.rank).collect::<Vec<_>>(), vec![1, 2]);
    }

    #[tokio::test]
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
                &SnapshotInput { stars: today, forks: 0, watchers: None, stars_today: Some(today) },
            )
            .await
            .unwrap();
        }
        let rows = trending(&pool, date, None, 100).await.unwrap();
        let rows: Vec<_> = rows.into_iter().filter(|r| r.full_name.starts_with("trend/")).collect();
        assert_eq!(rows[0].full_name, "trend/t2");
        assert_eq!(rows[0].stars_today, Some(50));
    }

    #[tokio::test]
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
        assert_eq!(latest_snapshot_date(&pool, Board::TopWatchers).await.unwrap(), None);
        for d in [d1, d2] {
            let id = upsert_repo(&pool, &repo("late/x", None), d).await.unwrap();
            upsert_snapshot(
                &pool,
                id,
                d,
                Board::TopWatchers,
                &SnapshotInput { stars: 1, forks: 0, watchers: None, stars_today: None },
            )
            .await
            .unwrap();
        }
        assert_eq!(latest_snapshot_date(&pool, Board::TopWatchers).await.unwrap(), Some(d2));
    }
}
