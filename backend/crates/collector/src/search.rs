use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    Stars,
    Forks,
}

impl Metric {
    pub fn as_str(self) -> &'static str {
        match self {
            Metric::Stars => "stars",
            Metric::Forks => "forks",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchRepo {
    pub full_name: String,
    pub html_url: String,
    pub description: Option<String>,
    pub language: Option<String>,
    pub stars: i32,
    pub forks: i32,
}

#[derive(Deserialize)]
struct SearchResponse {
    items: Vec<SearchItem>,
}

#[derive(Deserialize)]
struct SearchItem {
    full_name: String,
    html_url: String,
    description: Option<String>,
    language: Option<String>,
    stargazers_count: i32,
    forks_count: i32,
}

pub async fn search_top(
    client: &reqwest::Client,
    base: &str,
    token: Option<&str>,
    lang: Option<&str>,
    metric: Metric,
    per_page: u32,
    pages: u32,
) -> anyhow::Result<Vec<SearchRepo>> {
    let mut out = Vec::new();
    let q = match lang {
        Some(l) => format!("language:{l}"),
        None => "is:public".to_string(),
    };
    let per_page_s = per_page.to_string();
    for page in 1..=pages {
        let page_s = page.to_string();
        let mut req = client
            .get(format!("{base}/search/repositories"))
            .header("Accept", "application/vnd.github+json")
            .query(&[
                ("q", q.as_str()),
                ("sort", metric.as_str()),
                ("per_page", per_page_s.as_str()),
                ("page", page_s.as_str()),
            ]);
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        let resp: SearchResponse = req.send().await?.error_for_status()?.json().await?;
        if resp.items.is_empty() {
            break;
        }
        out.extend(resp.items.into_iter().map(|it| SearchRepo {
            full_name: it.full_name,
            html_url: it.html_url,
            description: it.description,
            language: it.language,
            stars: it.stargazers_count,
            forks: it.forks_count,
        }));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn page_body(names: &[&str]) -> String {
        let items: Vec<String> = names
            .iter()
            .map(|n| format!(
                r#"{{"full_name":"{n}","html_url":"https://github.com/{n}","description":"d","language":"Python","stargazers_count":100,"forks_count":10}}"#
            ))
            .collect();
        format!(r#"{{"total_count":{},"items":[{}]}}"#, names.len(), items.join(","))
    }

    #[tokio::test]
    async fn sends_auth_query_params_and_parses_items() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search/repositories"))
            .and(query_param("q", "language:Python"))
            .and(query_param("sort", "stars"))
            .and(query_param("per_page", "100"))
            .and(query_param("page", "1"))
            .and(header("authorization", "Bearer t0k3n"))
            .respond_with(ResponseTemplate::new(200).set_body_string(page_body(&["a/x", "b/y"])))
            .expect(1)
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let repos = search_top(&client, &server.uri(), Some("t0k3n"), Some("Python"), Metric::Stars, 100, 1)
            .await
            .unwrap();
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].full_name, "a/x");
        assert_eq!(repos[0].stars, 100);
        assert_eq!(repos[0].forks, 10);
    }

    #[tokio::test]
    async fn paginates_until_empty_page() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search/repositories"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_string(page_body(&["a/x"])))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/search/repositories"))
            .and(query_param("page", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_string(page_body(&[])))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let repos = search_top(&client, &server.uri(), None, None, Metric::Forks, 1, 5).await.unwrap();
        assert_eq!(repos.len(), 1);
    }

    #[tokio::test]
    async fn none_lang_uses_is_public_query() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search/repositories"))
            .and(query_param("q", "is:public"))
            .and(query_param("sort", "stars"))
            .respond_with(ResponseTemplate::new(200).set_body_string(page_body(&["a/x"])))
            .expect(1)
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let repos = search_top(&client, &server.uri(), None, None, Metric::Stars, 100, 1)
            .await
            .unwrap();
        assert_eq!(repos.len(), 1);
    }

    #[tokio::test]
    async fn non_2xx_is_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        assert!(search_top(&client, &server.uri(), None, None, Metric::Stars, 100, 1).await.is_err());
    }
}
