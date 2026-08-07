use crate::enrich::{self, ENRICH_INTERVAL};
use crate::graphql::{fetch_watchers, WatchTarget};
use crate::search::{search_top, Metric};
use crate::store::{
    apply_enrichment, list_repos_needing_enrichment, split_full_name, store_top_rows,
    store_trending_rows, TopEntry,
};
use crate::trending::fetch_trending;
use chrono::Utc;
use ght_core::config::Settings;
use ght_core::models::{Board, RepoInput, SnapshotInput};
use ght_core::store as core_store;
use sqlx::PgPool;
use std::time::Duration;

pub struct Collector {
    pub pool: PgPool,
    pub http: reqwest::Client,
    pub settings: Settings,
    pub github_base: String,
    pub api_base: String,
}

#[derive(Debug, Default)]
pub struct Report {
    pub ok: usize,
    pub failed: usize,
}

// GitHub 搜索 API 认证限额 30 次/分钟。1.5s 间隔峰值可达 40 次/分钟，
// 且 watch 候选阶段每语言连发 5 页会触发二级限流（abuse detection, 403）。
// 提到 2.5s（≈24 次/分钟）留出余量，确保含 watch 候选的完整抓取不被限流。
const SEARCH_INTERVAL: Duration = Duration::from_millis(2500);
const TRENDING_INTERVAL: Duration = Duration::from_millis(2000);
// 实测：stars/forks 阶段的连续搜索会触发二级限流，余波持续到 watch 候选
// 阶段开头（前几个语言必 403，之后窗口恢复）。进入 watch 阶段前先冷却，
// 让二级限流窗口过去，避免候选池整段丢失。
const WATCH_PHASE_COOLDOWN: Duration = Duration::from_secs(45);

impl Collector {
    fn langs(&self) -> Vec<Option<String>> {
        std::iter::once(None)
            .chain(self.settings.languages.iter().map(|l| Some(l.clone())))
            .collect()
    }

    pub async fn collect_once(&self) -> Report {
        let today = Utc::now().date_naive();
        let mut report = Report::default();
        let token = self.settings.github_token.as_deref();

        // 1. 总榜 stars / forks
        for lang in self.langs() {
            for metric in [Metric::Stars, Metric::Forks] {
                let board = match metric {
                    Metric::Stars => Board::TopStars,
                    Metric::Forks => Board::TopForks,
                };
                match search_top(&self.http, &self.api_base, token, lang.as_deref(), metric, 100, 1).await {
                    Ok(rows) => {
                        let entries: Vec<TopEntry> = rows
                            .into_iter()
                            .map(|r| TopEntry {
                                repo_full_name: r.full_name,
                                html_url: r.html_url,
                                description: r.description,
                                language: r.language,
                                stars: r.stars,
                                forks: r.forks,
                                watchers: None,
                                topics: r.topics,
                            })
                            .collect();
                        match store_top_rows(&self.pool, today, board, &entries).await {
                            Ok(n) => report.ok += n,
                            Err(e) => {
                                tracing::warn!(error = %e, board = board.as_str(), "store failed");
                                report.failed += 1;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, lang = ?lang, metric = ?metric, "search failed");
                        report.failed += 1;
                    }
                }
                tokio::time::sleep(SEARCH_INTERVAL).await;
            }
        }

        // 2. 总榜 watchers（候选池 = star top500，需 token）
        match token {
            None => tracing::warn!("GITHUB_TOKEN not set; skipping watch board"),
            Some(token) => {
                tokio::time::sleep(WATCH_PHASE_COOLDOWN).await;
                for lang in self.langs() {
                    let pool_candidates = search_top(&self.http, &self.api_base, Some(token), lang.as_deref(), Metric::Stars, 100, 5).await;
                    match pool_candidates {
                        Ok(candidates) => {
                            let targets: Vec<WatchTarget> = candidates
                                .iter()
                                .map(|c| {
                                    let (o, n) = split_full_name(&c.full_name);
                                    WatchTarget { owner: o.to_string(), name: n.to_string() }
                                })
                                .collect();
                            match fetch_watchers(&self.http, &self.api_base, token, &targets, 50).await {
                                Ok(watchers) => {
                                    let mut scored: Vec<TopEntry> = candidates
                                        .into_iter()
                                        .filter_map(|c| {
                                            let w = *watchers.get(&c.full_name)?;
                                            Some(TopEntry {
                                                repo_full_name: c.full_name,
                                                html_url: c.html_url,
                                                description: c.description,
                                                language: c.language,
                                                stars: c.stars,
                                                forks: c.forks,
                                                watchers: Some(w),
                                                topics: c.topics,
                                            })
                                        })
                                        .collect();
                                    scored.sort_by(|a, b| b.watchers.unwrap_or(0).cmp(&a.watchers.unwrap_or(0)));
                                    scored.truncate(100);
                                    match store_top_rows(&self.pool, today, Board::TopWatchers, &scored).await {
                                        Ok(n) => report.ok += n,
                                        Err(e) => {
                                            tracing::warn!(error = %e, "watch store failed");
                                            report.failed += 1;
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, lang = ?lang, "graphql failed");
                                    report.failed += 1;
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, lang = ?lang, "watch candidate search failed");
                            report.failed += 1;
                        }
                    }
                    tokio::time::sleep(SEARCH_INTERVAL).await;
                }
            }
        }

        // 3. 趋势榜
        for lang in self.langs() {
            match fetch_trending(&self.http, &self.github_base, lang.as_deref()).await {
                Ok(rows) => match store_trending_rows(&self.pool, today, &rows).await {
                    Ok(n) => report.ok += n,
                    Err(e) => {
                        tracing::warn!(error = %e, "trending store failed");
                        report.failed += 1;
                    }
                },
                Err(e) => {
                    tracing::warn!(error = %e, lang = ?lang, "trending fetch failed");
                    report.failed += 1;
                }
            }
            tokio::time::sleep(TRENDING_INTERVAL).await;
        }

        // 4. Enrich topics/languages for today's board repos (failures do not fail the board).
        self.enrich_today_repos(today).await;

        // 5. Daily scan of user-tracked repos → tracked_daily snapshots (failures isolated).
        self.scan_tracked_repos(today).await;

        // 6. 清理过期 refresh token
        if let Err(e) = core_store::cleanup_expired_refresh_tokens(&self.pool).await {
            tracing::warn!(error = %e, "refresh token cleanup failed");
        }

        report
    }

    /// Batch-fill missing topics / language shares for repos on today's snapshots.
    /// Search-path topics are already written at store time; this covers languages and
    /// trending-only (empty topics) repos. Enrichment errors are logged only.
    async fn enrich_today_repos(&self, today: chrono::NaiveDate) {
        let needs = match list_repos_needing_enrichment(&self.pool, today).await {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!(error = %e, "list repos needing enrichment failed");
                return;
            }
        };
        if needs.is_empty() {
            return;
        }
        tracing::info!(count = needs.len(), "enriching repos topics/languages");
        let token = self.settings.github_token.as_deref();

        for need in needs {
            let mut topics = Vec::new();
            let mut language_names = Vec::new();
            let mut languages_json = ght_core::models::RepoInput::languages_empty();
            let mut any_ok = false;

            if need.needs_languages {
                match enrich::fetch_languages(
                    &self.http,
                    &self.api_base,
                    token,
                    &need.owner,
                    &need.name,
                )
                .await
                {
                    Ok((shares, names)) => {
                        languages_json = enrich::languages_json(&shares);
                        language_names = names;
                        any_ok = true;
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            repo = %need.full_name,
                            "fetch languages failed; continuing"
                        );
                    }
                }
                tokio::time::sleep(ENRICH_INTERVAL).await;
            }

            if need.needs_topics {
                match enrich::fetch_repo_topics(
                    &self.http,
                    &self.api_base,
                    token,
                    &need.owner,
                    &need.name,
                )
                .await
                {
                    Ok(t) => {
                        topics = t;
                        any_ok = true;
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            repo = %need.full_name,
                            "fetch topics failed; continuing"
                        );
                    }
                }
                tokio::time::sleep(ENRICH_INTERVAL).await;
            }

            if !any_ok {
                continue;
            }

            if let Err(e) = apply_enrichment(
                &self.pool,
                today,
                &need,
                topics,
                language_names,
                languages_json,
            )
            .await
            {
                tracing::warn!(
                    error = %e,
                    repo = %need.full_name,
                    "apply enrichment failed; continuing"
                );
            }
        }
    }

    /// Snapshot every distinct user-tracked repo onto `board=tracked_daily`.
    /// Refreshes repo meta + languages. Per-repo failures are logged only.
    async fn scan_tracked_repos(&self, today: chrono::NaiveDate) {
        let names = match core_store::list_all_tracked_full_names(&self.pool).await {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!(error = %e, "list tracked full_names failed");
                return;
            }
        };
        if names.is_empty() {
            return;
        }
        tracing::info!(count = names.len(), "scanning tracked repos for tracked_daily");
        let token = self.settings.github_token.as_deref();

        for full_name in names {
            let (owner, name) = split_full_name(&full_name);
            if owner.is_empty() || name.is_empty() {
                tracing::warn!(repo = %full_name, "invalid tracked full_name; skipping");
                continue;
            }

            let details = match enrich::fetch_repo_details(
                &self.http,
                &self.api_base,
                token,
                owner,
                name,
            )
            .await
            {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        repo = %full_name,
                        "fetch tracked repo details failed; continuing"
                    );
                    continue;
                }
            };
            tokio::time::sleep(ENRICH_INTERVAL).await;

            // Languages best-effort: empty on failure so upsert preserves existing.
            let (shares, language_names) = match enrich::fetch_languages(
                &self.http,
                &self.api_base,
                token,
                owner,
                name,
            )
            .await
            {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        repo = %full_name,
                        "fetch tracked languages failed; continuing with empty"
                    );
                    (vec![], vec![])
                }
            };
            tokio::time::sleep(ENRICH_INTERVAL).await;

            let languages_json = enrich::languages_json(&shares);
            let repo = RepoInput {
                full_name: details.full_name.clone(),
                owner: details.owner,
                name: details.name,
                html_url: details.html_url,
                language: details
                    .language
                    .clone()
                    .or_else(|| language_names.first().cloned()),
                description: details.description,
                topics: details.topics,
                languages_json,
                language_names,
            };

            let repo_id = match core_store::upsert_repo(&self.pool, &repo, today).await {
                Ok(id) => id,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        repo = %full_name,
                        "upsert tracked repo failed; continuing"
                    );
                    continue;
                }
            };

            if let Err(e) = core_store::upsert_indexed_snapshots(
                &self.pool,
                repo_id,
                today,
                &SnapshotInput {
                    stars: details.stars,
                    forks: details.forks,
                    watchers: details.watchers,
                    stars_today: None,
                },
            )
            .await
            {
                tracing::warn!(
                    error = %e,
                    repo = %full_name,
                    "upsert tracked_daily snapshot failed; continuing"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ght_core::{db, store as core_store};
    use serde_json::json;
    use serial_test::serial;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn test_pool() -> PgPool {
        let url = std::env::var("DATABASE_URL_TEST_COLLECTOR")
            .unwrap_or_else(|_| "postgres://postgres@localhost:5432/ghtrending_test_collector".into());
        let pool = db::pg_pool(&url).await.expect("test db unreachable; run `make db`");
        db::migrate(&pool).await.unwrap();
        sqlx::query(
            "TRUNCATE repos, snapshots, users, invite_codes, refresh_tokens, user_tracked_repos",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    fn trending_body() -> String {
        include_str!("../tests/fixtures/trending.html").to_string()
    }

    fn search_body(names: &[&str], stars: i32) -> String {
        let items: Vec<String> = names
            .iter()
            .map(|n| format!(
                r#"{{"full_name":"{n}","html_url":"https://github.com/{n}","description":null,"language":"Python","stargazers_count":{stars},"forks_count":3,"topics":["AI","llm"]}}"#
            ))
            .collect();
        format!(r#"{{"total_count":{},"items":[{}]}}"#, names.len(), items.join(","))
    }

    async fn mount_all(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/search/repositories"))
            .respond_with(ResponseTemplate::new(200).set_body_string(search_body(&["a/top"], 999)))
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"q0": {"watchers": {"totalCount": 77}}}})))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex("/trending.*"))
            .respond_with(ResponseTemplate::new(200).set_body_string(trending_body()))
            .mount(server)
            .await;
        // Enrichment: languages + repo meta for topics (trending-only / missing).
        Mock::given(method("GET"))
            .and(path_regex(r"^/repos/.+/languages$"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(r#"{"Rust":900,"Python":100}"#),
            )
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/repos/[^/]+/[^/]+$"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"full_name":"mock/repo","html_url":"https://github.com/mock/repo","topics":["trending","demo"],"private":false,"stargazers_count":0,"forks_count":0}"#,
            ))
            .mount(server)
            .await;
    }

    use wiremock::matchers::path_regex;

    fn test_collector(pool: PgPool, uri: String, token: Option<String>) -> Collector {
        let settings = ght_core::config::Settings::from_map(|k| match k {
            "DATABASE_URL" => Some("postgres://unused".into()),
            "JWT_SECRET" => Some("unused".into()),
            "LANGUAGES" => Some("Rust".into()),
            "GITHUB_TOKEN" => token.clone(),
            _ => None,
        })
        .unwrap();
        Collector {
            pool,
            http: reqwest::Client::builder().user_agent("gh-trending-collector/0.1").build().unwrap(),
            settings,
            github_base: uri.clone(),
            api_base: uri,
        }
    }

    #[tokio::test]
    #[serial]
    async fn collect_once_writes_all_boards_and_is_idempotent() {
        let server = MockServer::start().await;
        mount_all(&server).await;
        let pool = test_pool().await;
        let collector = test_collector(pool.clone(), server.uri(), Some("t0k3n".into()));

        // 集成测试走真实 sleep 太慢：本测试接受 ~14s（(2 langs × 2 metrics + 2 langs watch + 2 langs trending) × 间隔）。
        // 若需加速可将 SEARCH_INTERVAL/TRENDING_INTERVAL 改为可配置字段；此处保持与生产一致。
        let report = collector.collect_once().await;
        assert!(report.failed == 0, "report: ok={} failed={}", report.ok, report.failed);

        let today = Utc::now().date_naive();
        let empty = ght_core::models::LeaderboardFilter::empty();
        assert_eq!(core_store::top_by_stars(&pool, today, empty, 100).await.unwrap().len(), 1);
        assert_eq!(core_store::top_by_forks(&pool, today, empty, 100).await.unwrap().len(), 1);
        let watch = core_store::top_by_watchers(&pool, today, empty, 100).await.unwrap();
        assert_eq!(watch.len(), 1);
        assert_eq!(watch[0].watchers, Some(77));
        assert_eq!(core_store::trending(&pool, today, empty, 100).await.unwrap().len(), 3);

        // 幂等：重跑行数不变，四 board 均断言
        let report2 = collector.collect_once().await;
        assert_eq!(report2.failed, 0);
        assert_eq!(core_store::top_by_stars(&pool, today, empty, 100).await.unwrap().len(), 1);
        assert_eq!(core_store::top_by_forks(&pool, today, empty, 100).await.unwrap().len(), 1);
        assert_eq!(core_store::top_by_watchers(&pool, today, empty, 100).await.unwrap().len(), 1);
        assert_eq!(core_store::trending(&pool, today, empty, 100).await.unwrap().len(), 3);
        assert_eq!(
            core_store::board_count(&pool, today, Board::TopStars).await.unwrap(),
            1
        );
        assert_eq!(
            core_store::board_count(&pool, today, Board::TopForks).await.unwrap(),
            1
        );
        assert_eq!(
            core_store::board_count(&pool, today, Board::TopWatchers).await.unwrap(),
            1
        );
        assert_eq!(
            core_store::board_count(&pool, today, Board::TrendingDaily).await.unwrap(),
            3
        );

        // Enrichment: search topics normalized; languages filled for board repos.
        let top = sqlx::query!(
            r#"SELECT topics AS "topics!", language_names AS "language_names!",
                      languages AS "languages!", last_enriched_at
               FROM repos WHERE full_name = 'a/top'"#
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(top.topics, vec!["ai".to_string(), "llm".to_string()]);
        assert_eq!(
            top.language_names,
            vec!["Rust".to_string(), "Python".to_string()]
        );
        assert!(top.languages.is_array());
        assert!(top.last_enriched_at.is_some());
    }

    #[tokio::test]
    #[serial]
    async fn missing_token_skips_watch_board_but_keeps_others() {
        let server = MockServer::start().await;
        mount_all(&server).await;
        let pool = test_pool().await;
        let collector = test_collector(pool.clone(), server.uri(), None);
        let report = collector.collect_once().await;
        assert!(report.failed == 0);
        let today = Utc::now().date_naive();
        let empty = ght_core::models::LeaderboardFilter::empty();
        assert_eq!(core_store::top_by_watchers(&pool, today, empty, 100).await.unwrap().len(), 0);
        assert_eq!(core_store::top_by_stars(&pool, today, empty, 100).await.unwrap().len(), 1);
    }

    #[tokio::test]
    #[serial]
    async fn enrich_failure_does_not_fail_public_board() {
        let server = MockServer::start().await;
        // Boards only — no languages/topics mocks; enrich 404s must not fail collect.
        Mock::given(method("GET"))
            .and(path("/search/repositories"))
            .respond_with(ResponseTemplate::new(200).set_body_string(search_body(&["a/top"], 999)))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"q0": {"watchers": {"totalCount": 77}}}})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex("/trending.*"))
            .respond_with(ResponseTemplate::new(200).set_body_string(trending_body()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/repos/"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let pool = test_pool().await;
        let collector = test_collector(pool.clone(), server.uri(), Some("t0k3n".into()));
        let report = collector.collect_once().await;
        assert_eq!(report.failed, 0, "enrich failure must not count as board failure");
        let today = Utc::now().date_naive();
        let empty = ght_core::models::LeaderboardFilter::empty();
        assert_eq!(core_store::top_by_stars(&pool, today, empty, 100).await.unwrap().len(), 1);
        // Search topics still written; languages stay empty after failed enrich.
        let top = sqlx::query!(
            r#"SELECT topics AS "topics!", language_names AS "language_names!"
               FROM repos WHERE full_name = 'a/top'"#
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(top.topics, vec!["ai".to_string(), "llm".to_string()]);
        assert!(top.language_names.is_empty());
    }

    #[tokio::test]
    #[serial]
    async fn empty_upsert_does_not_wipe_existing_enrichment() {
        let pool = test_pool().await;
        let today = Utc::now().date_naive();
        let entry = TopEntry {
            repo_full_name: "wipe/test".into(),
            html_url: "https://github.com/wipe/test".into(),
            description: Some("d".into()),
            language: Some("Rust".into()),
            stars: 1,
            forks: 0,
            watchers: None,
            topics: vec!["ai".into()],
        };
        store_top_rows(&pool, today, Board::TopStars, &[entry.clone()])
            .await
            .unwrap();
        // Simulate successful enrich write.
        let need = crate::store::EnrichmentNeed {
            full_name: "wipe/test".into(),
            owner: "wipe".into(),
            name: "test".into(),
            html_url: entry.html_url.clone(),
            language: entry.language.clone(),
            description: entry.description.clone(),
            needs_topics: false,
            needs_languages: true,
        };
        apply_enrichment(
            &pool,
            today,
            &need,
            vec![],
            vec!["Rust".into()],
            serde_json::json!([{"name":"Rust","pct":100.0,"bytes":10}]),
        )
        .await
        .unwrap();

        // Second board store with empty topics/languages must preserve enrichment.
        let again = TopEntry {
            topics: vec![],
            stars: 2,
            ..entry
        };
        store_top_rows(&pool, today, Board::TopForks, &[again])
            .await
            .unwrap();

        let row = sqlx::query!(
            r#"SELECT topics AS "topics!", language_names AS "language_names!"
               FROM repos WHERE full_name = 'wipe/test'"#
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.topics, vec!["ai".to_string()]);
        assert_eq!(row.language_names, vec!["Rust".to_string()]);
    }

    #[tokio::test]
    #[serial]
    async fn scan_tracked_writes_tracked_daily_snapshot() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/tracked/only"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "full_name": "tracked/only",
                "html_url": "https://github.com/tracked/only",
                "description": "user tracked repo",
                "language": "Rust",
                "topics": ["AI", "tools"],
                "private": false,
                "stargazers_count": 1234,
                "forks_count": 56,
                "subscribers_count": 78,
                "owner": {"login": "tracked"},
                "name": "only"
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/tracked/only/languages"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(r#"{"Rust":900,"TS":100}"#),
            )
            .expect(1)
            .mount(&server)
            .await;

        let pool = test_pool().await;
        let today = Utc::now().date_naive();
        let uid: i64 = sqlx::query_scalar(
            "INSERT INTO users (username, password_hash) VALUES ('col_track_u', 'h') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        // Seed repo + track row without any snapshot (pending until scan).
        let repo_id = core_store::upsert_repo(
            &pool,
            &RepoInput {
                full_name: "tracked/only".into(),
                owner: "tracked".into(),
                name: "only".into(),
                html_url: "https://github.com/tracked/only".into(),
                language: None,
                description: None,
                topics: vec![],
                languages_json: RepoInput::languages_empty(),
                language_names: vec![],
            },
            today,
        )
        .await
        .unwrap();
        core_store::track_repo(&pool, uid, repo_id).await.unwrap();

        let collector = test_collector(pool.clone(), server.uri(), Some("t0k3n".into()));
        collector.scan_tracked_repos(today).await;

        // Indexed onto tracked_daily + public metric boards (stars/forks/watchers).
        let snaps = sqlx::query!(
            r#"SELECT s.stars, s.forks, s.watchers, s.board AS "board!"
               FROM snapshots s
               JOIN repos r ON r.id = s.repo_id
               WHERE r.full_name = 'tracked/only' AND s.snapshot_date = $1
               ORDER BY s.board"#,
            today
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let boards: Vec<_> = snaps.iter().map(|s| s.board.as_str()).collect();
        assert!(boards.contains(&"tracked_daily"));
        assert!(boards.contains(&"top_stars"));
        assert!(boards.contains(&"top_forks"));
        assert!(boards.contains(&"top_watchers"));
        for s in &snaps {
            assert_eq!(s.stars, 1234);
            assert_eq!(s.forks, 56);
            assert_eq!(s.watchers, Some(78));
        }

        // Meta + languages refreshed.
        let repo = sqlx::query!(
            r#"SELECT description, topics AS "topics!", language_names AS "language_names!"
               FROM repos WHERE full_name = 'tracked/only'"#
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(repo.description.as_deref(), Some("user tracked repo"));
        assert_eq!(repo.topics, vec!["ai".to_string(), "tools".to_string()]);
        assert_eq!(
            repo.language_names,
            vec!["Rust".to_string(), "TS".to_string()]
        );

        // User-tracked repos are first-class on public metric boards.
        assert_eq!(
            core_store::board_count(&pool, today, Board::TopStars)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            core_store::board_count(&pool, today, Board::TrackedDaily)
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    #[serial]
    async fn tracked_scan_failure_does_not_fail_public_boards() {
        let server = MockServer::start().await;
        // Public boards only; all /repos/* 404 so tracked scan + enrich fail isolated.
        Mock::given(method("GET"))
            .and(path("/search/repositories"))
            .respond_with(ResponseTemplate::new(200).set_body_string(search_body(&["a/top"], 999)))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"q0": {"watchers": {"totalCount": 77}}}})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex("/trending.*"))
            .respond_with(ResponseTemplate::new(200).set_body_string(trending_body()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/repos/"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let pool = test_pool().await;
        let today = Utc::now().date_naive();
        let uid: i64 = sqlx::query_scalar(
            "INSERT INTO users (username, password_hash) VALUES ('col_track_fail', 'h') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let repo_id = core_store::upsert_repo(
            &pool,
            &RepoInput {
                full_name: "tracked/fail".into(),
                owner: "tracked".into(),
                name: "fail".into(),
                html_url: "https://github.com/tracked/fail".into(),
                language: None,
                description: None,
                topics: vec![],
                languages_json: RepoInput::languages_empty(),
                language_names: vec![],
            },
            today,
        )
        .await
        .unwrap();
        core_store::track_repo(&pool, uid, repo_id).await.unwrap();

        let collector = test_collector(pool.clone(), server.uri(), Some("t0k3n".into()));
        let report = collector.collect_once().await;
        assert_eq!(
            report.failed, 0,
            "tracked fetch failure must not fail collect: ok={} failed={}",
            report.ok, report.failed
        );
        let empty = ght_core::models::LeaderboardFilter::empty();
        assert_eq!(
            core_store::top_by_stars(&pool, today, empty, 100)
                .await
                .unwrap()
                .len(),
            1
        );
        // No tracked_daily snapshot written for the failing repo.
        assert_eq!(
            core_store::board_count(&pool, today, Board::TrackedDaily)
                .await
                .unwrap(),
            0
        );
    }
}
