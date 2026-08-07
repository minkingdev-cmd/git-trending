use std::collections::HashMap;
use std::time::Duration;

use ght_core::models::LanguageShare;

/// Minimum delay between per-repo enrich HTTP calls (languages / topics).
pub const ENRICH_INTERVAL: Duration = Duration::from_millis(200);

/// Lowercase, trim, drop empty, sort, dedupe.
pub fn normalize_topics(raw: &[String]) -> Vec<String> {
    let mut out: Vec<String> = raw
        .iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Convert GitHub language byte map → shares (pct = bytes/total*100) and name list.
/// Both ordered by bytes descending (then name ascending for ties).
pub fn shares_from_language_map(map: HashMap<String, i64>) -> (Vec<LanguageShare>, Vec<String>) {
    let total: i64 = map.values().copied().sum();
    let mut pairs: Vec<(String, i64)> = map.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let shares: Vec<LanguageShare> = pairs
        .into_iter()
        .map(|(name, bytes)| {
            let pct = if total > 0 {
                (bytes as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            LanguageShare {
                name,
                pct,
                bytes: Some(bytes),
            }
        })
        .collect();
    let names: Vec<String> = shares.iter().map(|s| s.name.clone()).collect();
    (shares, names)
}

/// Serialize language shares for `repos.languages` JSONB.
pub fn languages_json(shares: &[LanguageShare]) -> serde_json::Value {
    serde_json::to_value(shares).unwrap_or_else(|_| serde_json::json!([]))
}

/// GET `{base}/repos/{owner}/{name}/languages` → shares + language_names.
pub async fn fetch_languages(
    client: &reqwest::Client,
    base: &str,
    token: Option<&str>,
    owner: &str,
    name: &str,
) -> anyhow::Result<(Vec<LanguageShare>, Vec<String>)> {
    let url = format!("{base}/repos/{owner}/{name}/languages");
    let mut req = client
        .get(&url)
        .header("Accept", "application/vnd.github+json");
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let map: HashMap<String, i64> = req.send().await?.error_for_status()?.json().await?;
    Ok(shares_from_language_map(map))
}

/// GET `{base}/repos/{owner}/{name}` → normalized topics (for trending-only / missing topics).
pub async fn fetch_repo_topics(
    client: &reqwest::Client,
    base: &str,
    token: Option<&str>,
    owner: &str,
    name: &str,
) -> anyhow::Result<Vec<String>> {
    let url = format!("{base}/repos/{owner}/{name}");
    let mut req = client
        .get(&url)
        .header("Accept", "application/vnd.github+json");
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    #[derive(serde::Deserialize)]
    struct RepoMeta {
        #[serde(default)]
        topics: Vec<String>,
    }
    let meta: RepoMeta = req.send().await?.error_for_status()?.json().await?;
    Ok(normalize_topics(&meta.topics))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn normalize_topics_lowercases_trims_dedupes_sorts() {
        let raw = vec![
            " AI ".into(),
            "llm".into(),
            "ai".into(),
            "".into(),
            "  ".into(),
            "Rust".into(),
        ];
        assert_eq!(
            normalize_topics(&raw),
            vec!["ai".to_string(), "llm".to_string(), "rust".to_string()]
        );
    }

    #[test]
    fn language_shares_sum_near_100() {
        let mut m = HashMap::new();
        m.insert("Rust".into(), 90);
        m.insert("Python".into(), 10);
        let (shares, names) = shares_from_language_map(m);
        assert_eq!(names, vec!["Rust", "Python"]); // sorted by bytes desc
        assert!((shares[0].pct - 90.0).abs() < 0.01);
        assert!((shares[1].pct - 10.0).abs() < 0.01);
        assert_eq!(shares[0].bytes, Some(90));
        let sum: f64 = shares.iter().map(|s| s.pct).sum();
        assert!((sum - 100.0).abs() < 0.01);
    }

    #[test]
    fn language_shares_empty_map() {
        let (shares, names) = shares_from_language_map(HashMap::new());
        assert!(shares.is_empty());
        assert!(names.is_empty());
    }

    #[test]
    fn language_shares_tie_break_by_name() {
        let mut m = HashMap::new();
        m.insert("Zebra".into(), 50);
        m.insert("Alpha".into(), 50);
        let (shares, names) = shares_from_language_map(m);
        assert_eq!(names, vec!["Alpha", "Zebra"]);
        assert!((shares[0].pct - 50.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn fetch_languages_parses_wiremock_body() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/o/n/languages"))
            .and(header("accept", "application/vnd.github+json"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(r#"{"Rust":900,"Python":100}"#),
            )
            .expect(1)
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let (shares, names) = fetch_languages(&client, &server.uri(), Some("tok"), "o", "n")
            .await
            .unwrap();
        assert_eq!(names, vec!["Rust", "Python"]);
        assert!((shares[0].pct - 90.0).abs() < 0.01);
        assert_eq!(shares[0].bytes, Some(900));
        assert!((shares[1].pct - 10.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn fetch_languages_non_2xx_is_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/o/n/languages"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        assert!(fetch_languages(&client, &server.uri(), None, "o", "n")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn fetch_repo_topics_normalizes() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/o/n"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"topics":["AI","llm","AI"]}"#),
            )
            .expect(1)
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let topics = fetch_repo_topics(&client, &server.uri(), None, "o", "n")
            .await
            .unwrap();
        assert_eq!(topics, vec!["ai".to_string(), "llm".to_string()]);
    }
}
