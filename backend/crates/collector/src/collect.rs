use crate::graphql::{fetch_watchers, WatchTarget};
use crate::search::{search_top, Metric};
use crate::store::{split_full_name, store_top_rows, store_trending_rows, TopEntry};
use crate::trending::fetch_trending;
use chrono::Utc;
use ght_core::config::Settings;
use ght_core::models::Board;
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

const SEARCH_INTERVAL: Duration = Duration::from_millis(1500);
const TRENDING_INTERVAL: Duration = Duration::from_millis(2000);

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

        // 4. 清理过期 refresh token
        if let Err(e) = core_store::cleanup_expired_refresh_tokens(&self.pool).await {
            tracing::warn!(error = %e, "refresh token cleanup failed");
        }

        report
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
        let url = std::env::var("DATABASE_URL_TEST")
            .unwrap_or_else(|_| "postgres://ght:ght@localhost:5433/ghtrending_test".into());
        let pool = db::pg_pool(&url).await.expect("test db unreachable; run `make db`");
        db::migrate(&pool).await.unwrap();
        sqlx::query("TRUNCATE repos, snapshots, users, invite_codes, refresh_tokens")
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
                r#"{{"full_name":"{n}","html_url":"https://github.com/{n}","description":null,"language":"Python","stargazers_count":{stars},"forks_count":3}}"#
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
        assert_eq!(core_store::top_by_stars(&pool, today, None, 100).await.unwrap().len(), 1);
        assert_eq!(core_store::top_by_forks(&pool, today, None, 100).await.unwrap().len(), 1);
        let watch = core_store::top_by_watchers(&pool, today, None, 100).await.unwrap();
        assert_eq!(watch.len(), 1);
        assert_eq!(watch[0].watchers, Some(77));
        assert_eq!(core_store::trending(&pool, today, None, 100).await.unwrap().len(), 3);

        // 幂等：重跑行数不变
        let report2 = collector.collect_once().await;
        assert!(report2.failed == 0);
        assert_eq!(core_store::top_by_stars(&pool, today, None, 100).await.unwrap().len(), 1);
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
        assert_eq!(core_store::top_by_watchers(&pool, today, None, 100).await.unwrap().len(), 0);
        assert_eq!(core_store::top_by_stars(&pool, today, None, 100).await.unwrap().len(), 1);
    }
}
