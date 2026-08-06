use scraper::{Html, Selector};

#[derive(Debug, Clone, PartialEq)]
pub struct TrendingRepo {
    pub full_name: String,
    pub description: Option<String>,
    pub language: Option<String>,
    pub stars: i32,
    pub forks: i32,
    pub stars_today: i32,
}

pub fn parse_trending_html(html: &str) -> Vec<TrendingRepo> {
    let doc = Html::parse_document(html);
    let row_sel = Selector::parse("article.Box-row").unwrap();
    let href_sel = Selector::parse("h2 a").unwrap();
    let desc_sel = Selector::parse("p.col-9").unwrap();
    let lang_sel = Selector::parse("[itemprop=programmingLanguage]").unwrap();
    let stars_sel = Selector::parse(r#"a[href$="/stargazers"]"#).unwrap();
    let forks_sel = Selector::parse(r#"a[href$="/forks"]"#).unwrap();
    let today_sel = Selector::parse("span.float-sm-right").unwrap();

    doc.select(&row_sel)
        .filter_map(|row| {
            let href = row.select(&href_sel).next()?.value().attr("href")?;
            let full_name = href.trim_start_matches('/').to_string();
            let stars = match row
                .select(&stars_sel)
                .next()
                .and_then(|el| parse_count(&el.text().collect::<String>()))
            {
                Some(s) => s,
                None => {
                    tracing::debug!(%full_name, "trending row missing stars; skipping");
                    return None;
                }
            };
            let forks = match row
                .select(&forks_sel)
                .next()
                .and_then(|el| parse_count(&el.text().collect::<String>()))
            {
                Some(f) => f,
                None => {
                    tracing::debug!(%full_name, "trending row missing forks; skipping");
                    return None;
                }
            };
            let stars_today = row
                .select(&today_sel)
                .next()
                .and_then(|el| parse_count(&el.text().collect::<String>()))
                .unwrap_or(0);
            Some(TrendingRepo {
                full_name,
                description: row
                    .select(&desc_sel)
                    .next()
                    .map(|el| el.text().collect::<String>().trim().to_string())
                    .filter(|s| !s.is_empty()),
                language: row
                    .select(&lang_sel)
                    .next()
                    .map(|el| el.text().collect::<String>().trim().to_string())
                    .filter(|s| !s.is_empty()),
                stars,
                forks,
                stars_today,
            })
        })
        .collect()
}

fn parse_count(text: &str) -> Option<i32> {
    let digits: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

pub async fn fetch_trending(
    client: &reqwest::Client,
    base: &str,
    lang: Option<&str>,
) -> anyhow::Result<Vec<TrendingRepo>> {
    let url = match lang {
        Some(l) => format!("{base}/trending/{l}?since=daily"),
        None => format!("{base}/trending?since=daily"),
    };
    let html = client.get(&url).send().await?.error_for_status()?.text().await?;
    Ok(parse_trending_html(&html))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture_rows() {
        let html = include_str!("../tests/fixtures/trending.html");
        let repos = parse_trending_html(html);
        assert_eq!(repos.len(), 3);

        assert_eq!(repos[0].full_name, "tensorflow/tensorflow");
        assert_eq!(repos[0].stars, 190_000);
        assert_eq!(repos[0].forks, 75_000);
        assert_eq!(repos[0].stars_today, 1_234);
        assert_eq!(repos[0].language.as_deref(), Some("Python"));
        assert!(repos[0].description.as_deref().unwrap().starts_with("An Open Source"));

        assert_eq!(repos[1].full_name, "oven-sh/bun");
        assert_eq!(repos[1].stars_today, 801);

        assert_eq!(repos[2].full_name, "awesome-lists/awesome");
        assert_eq!(repos[2].language, None);
        assert_eq!(repos[2].description, None);
        assert_eq!(repos[2].stars_today, 0);
    }

    #[tokio::test]
    async fn fetch_parses_response_and_sends_user_agent() {
        use wiremock::matchers::{header, method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/trending/rust"))
            .and(query_param("since", "daily"))
            .and(header("user-agent", "gh-trending-collector/0.1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(include_str!("../tests/fixtures/trending.html")),
            )
            .expect(1)
            .mount(&server)
            .await;

        let client = reqwest::Client::builder()
            .user_agent("gh-trending-collector/0.1")
            .build()
            .unwrap();
        let repos = fetch_trending(&client, &server.uri(), Some("rust")).await.unwrap();
        assert_eq!(repos.len(), 3);
    }

    #[tokio::test]
    async fn fetch_all_languages_path() {
        use wiremock::matchers::{header, method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/trending"))
            .and(query_param("since", "daily"))
            .and(header("user-agent", "gh-trending-collector/0.1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(include_str!("../tests/fixtures/trending.html")),
            )
            .expect(1)
            .mount(&server)
            .await;
        let client = reqwest::Client::builder()
            .user_agent("gh-trending-collector/0.1")
            .build()
            .unwrap();
        let repos = fetch_trending(&client, &server.uri(), None).await.unwrap();
        assert_eq!(repos.len(), 3);
    }
}
