use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct WatchTarget {
    pub owner: String,
    pub name: String,
}

pub fn build_watchers_query(batch: &[WatchTarget]) -> String {
    let fields: Vec<String> = batch
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.owner.contains('"') && !t.name.contains('"'))
        .map(|(i, t)| {
            format!(
                r#"q{i}: repository(owner: "{}", name: "{}") {{ watchers {{ totalCount }} }}"#,
                t.owner, t.name
            )
        })
        .collect();
    format!("query {{ {} }}", fields.join(" "))
}

pub fn parse_watchers(resp: &serde_json::Value, batch: &[WatchTarget]) -> Vec<(String, i32)> {
    let data = resp.get("data").unwrap_or(&serde_json::Value::Null);
    batch
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let count = data
                .get(format!("q{i}"))?
                .get("watchers")?
                .get("totalCount")?
                .as_i64()?;
            Some((format!("{}/{}", t.owner, t.name), count as i32))
        })
        .collect()
}

pub async fn fetch_watchers(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    targets: &[WatchTarget],
    batch_size: usize,
) -> anyhow::Result<HashMap<String, i32>> {
    let mut map = HashMap::new();
    for chunk in targets.chunks(batch_size.max(1)) {
        let body = serde_json::json!({ "query": build_watchers_query(chunk) });
        let resp: serde_json::Value = client
            .post(format!("{base}/graphql"))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        map.extend(parse_watchers(&resp, chunk));
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn builds_aliased_query() {
        let q = build_watchers_query(&[
            WatchTarget { owner: "a".into(), name: "x".into() },
            WatchTarget { owner: "b".into(), name: "y".into() },
        ]);
        assert!(q.contains(r#"q0: repository(owner: "a", name: "x")"#));
        assert!(q.contains(r#"q1: repository(owner: "b", name: "y")"#));
        assert!(q.starts_with("query {"));
    }

    #[test]
    fn parse_skips_null_repositories() {
        let batch = vec![
            WatchTarget { owner: "a".into(), name: "x".into() },
            WatchTarget { owner: "gone".into(), name: "repo".into() },
        ];
        let resp = json!({"data": {"q0": {"watchers": {"totalCount": 42}}, "q1": null}});
        let parsed = parse_watchers(&resp, &batch);
        assert_eq!(parsed, vec![("a/x".to_string(), 42)]);
    }

    #[tokio::test]
    async fn fetch_batches_and_sends_bearer() {
        let server = MockServer::start().await;
        let body1 = json!({"query": build_watchers_query(&[
            WatchTarget { owner: "a".into(), name: "x".into() },
            WatchTarget { owner: "b".into(), name: "y".into() },
        ])});
        let body2 = json!({"query": build_watchers_query(&[
            WatchTarget { owner: "c".into(), name: "z".into() },
        ])});
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(header("authorization", "Bearer t0k3n"))
            .and(body_json(&body1))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"data": {"q0": {"watchers": {"totalCount": 1}}, "q1": {"watchers": {"totalCount": 2}}}}),
            ))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(body_json(&body2))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"data": {"q0": {"watchers": {"totalCount": 3}}}}),
            ))
            .mount(&server)
            .await;

        let targets: Vec<WatchTarget> = ["a/x", "b/y", "c/z"]
            .iter()
            .map(|s| {
                let (o, n) = s.split_once('/').unwrap();
                WatchTarget { owner: (*o).into(), name: (*n).into() }
            })
            .collect();
        let client = reqwest::Client::new();
        let map = fetch_watchers(&client, &server.uri(), "t0k3n", &targets, 2).await.unwrap();
        assert_eq!(map.len(), 3);
        assert_eq!(map["c/z"], 3);
    }
}
