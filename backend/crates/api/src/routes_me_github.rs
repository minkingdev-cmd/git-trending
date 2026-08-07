//! User GitHub PAT: save (encrypted) and clear.
//!
//! - `PUT  /api/me/github-token` — body `{ "token": "..." }`; never echoes the token
//! - `DELETE /api/me/github-token` — clears stored ciphertext

use crate::auth::extract::RequireAuth;
use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::put;
use axum::{Json, Router};
use ght_core::crypto;
use ght_core::users;
use serde::{Deserialize, Serialize};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/me/github-token",
            put(put_github_token).delete(delete_github_token),
        )
}

#[derive(Debug, Deserialize)]
pub struct PutGithubTokenReq {
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct GithubTokenStatusResp {
    pub has_github_token: bool,
}

fn bad_request(msg: &str) -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": msg })),
    )
        .into_response()
}

/// Validate a PAT against GitHub `GET /rate_limit` (uses `settings.github_api_base`).
async fn validate_github_token(api_base: &str, token: &str) -> Result<(), StatusCode> {
    let client = reqwest::Client::builder()
        .user_agent("gh-trending-api/0.1")
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let url = format!("{}/rate_limit", api_base.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "github token validation request failed");
            StatusCode::BAD_GATEWAY
        })?;
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(StatusCode::BAD_REQUEST);
    }
    tracing::warn!(%status, "github token validation unexpected status");
    Err(StatusCode::BAD_GATEWAY)
}

async fn put_github_token(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
    Json(body): Json<PutGithubTokenReq>,
) -> impl IntoResponse {
    let token = body.token.trim();
    if token.is_empty() {
        return bad_request("token is required");
    }

    match validate_github_token(&state.settings.github_api_base, token).await {
        Ok(()) => {}
        Err(StatusCode::BAD_REQUEST) => {
            return bad_request("invalid github token");
        }
        Err(code) => {
            return (
                code,
                Json(serde_json::json!({ "error": "token validation failed" })),
            )
                .into_response();
        }
    }

    let ciphertext = match crypto::encrypt_token(&state.settings.token_encryption_key, token) {
        Ok(c) => c,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    if users::set_user_github_token(&state.pool, claims.sub, &ciphertext)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Never echo the token — only the presence flag.
    (
        StatusCode::OK,
        Json(GithubTokenStatusResp {
            has_github_token: true,
        }),
    )
        .into_response()
}

async fn delete_github_token(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
) -> impl IntoResponse {
    if users::clear_user_github_token(&state.pool, claims.sub)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    (
        StatusCode::OK,
        Json(GithubTokenStatusResp {
            has_github_token: false,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::tokens;
    use crate::state::AppState;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use ght_core::{db, users};
    use serial_test::serial;
    use tower::ServiceExt;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn test_state(github_base: Option<&str>) -> AppState {
        let url = std::env::var("DATABASE_URL_TEST_API")
            .or_else(|_| std::env::var("DATABASE_URL_TEST"))
            .unwrap_or_else(|_| "postgres://postgres@localhost:5432/ghtrending_test_api".into());
        let pool = db::pg_pool(&url)
            .await
            .expect("test db unreachable; run `make db`");
        db::migrate(&pool).await.unwrap();
        sqlx::query("TRUNCATE users, invite_codes, refresh_tokens")
            .execute(&pool)
            .await
            .unwrap();
        let base = github_base.map(|s| s.to_string());
        let settings = ght_core::config::Settings::from_map(|k| match k {
            "DATABASE_URL" => Some(url.clone()),
            "JWT_SECRET" => Some("test-secret".into()),
            "GITHUB_API_BASE" => base.clone(),
            _ => None,
        })
        .unwrap();
        AppState::new(pool, settings)
    }

    async fn create_user(state: &AppState, username: &str) -> i64 {
        users::create_user(&state.pool, username, "hash", None)
            .await
            .unwrap()
    }

    fn auth_cookie(state: &AppState, user_id: i64, username: &str) -> String {
        let jwt = tokens::issue_access(&state.settings.jwt_secret, user_id, username).unwrap();
        format!("access_token={jwt}")
    }

    async fn call(
        state: AppState,
        method: &str,
        uri: &str,
        cookie: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, String) {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(c) = cookie {
            builder = builder.header("cookie", c);
        }
        let req_body = if let Some(b) = body {
            builder = builder.header("content-type", "application/json");
            Body::from(serde_json::to_vec(&b).unwrap())
        } else {
            Body::empty()
        };
        let resp = crate::build_router(state)
            .oneshot(builder.body(req_body).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    async fn mount_rate_limit_ok(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/rate_limit"))
            .and(header("Authorization", "Bearer ghp_valid_token_abc"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"resources":{"core":{"limit":5000,"remaining":4999,"reset":0}}}"#,
            ))
            .mount(server)
            .await;
    }

    async fn mount_rate_limit_unauthorized(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/rate_limit"))
            .respond_with(ResponseTemplate::new(401).set_body_string(r#"{"message":"Bad credentials"}"#))
            .mount(server)
            .await;
    }

    #[tokio::test]
    #[serial]
    async fn put_delete_github_token_and_me_flag() {
        let server = MockServer::start().await;
        mount_rate_limit_ok(&server).await;

        let state = test_state(Some(&server.uri())).await;
        let uid = create_user(&state, "pat_user").await;
        let cookie = auth_cookie(&state, uid, "pat_user");

        // me: no token yet
        let (status, body) = call(state.clone(), "GET", "/api/auth/me", Some(&cookie), None).await;
        assert_eq!(status, StatusCode::OK, "body={body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["has_github_token"], false);
        assert_eq!(v["username"], "pat_user");

        // empty token rejected
        let (status, body) = call(
            state.clone(),
            "PUT",
            "/api/me/github-token",
            Some(&cookie),
            Some(serde_json::json!({"token": "   "})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body={body}");
        assert!(body.contains("token is required"));

        // unauthenticated rejected
        let (status, _) = call(
            state.clone(),
            "PUT",
            "/api/me/github-token",
            None,
            Some(serde_json::json!({"token": "ghp_valid_token_abc"})),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // put valid token
        let secret = "ghp_valid_token_abc";
        let (status, body) = call(
            state.clone(),
            "PUT",
            "/api/me/github-token",
            Some(&cookie),
            Some(serde_json::json!({"token": format!("  {secret}  ")})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body={body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["has_github_token"], true);
        // never echo token
        assert!(!body.contains(secret));
        assert!(!body.contains("ghp_"));

        // stored encrypted (not plaintext)
        assert!(users::user_has_github_token(&state.pool, uid).await.unwrap());
        let blob = users::get_user_github_token_ciphertext(&state.pool, uid)
            .await
            .unwrap()
            .expect("ciphertext");
        assert!(!blob.is_empty());
        let plain = String::from_utf8_lossy(&blob);
        assert!(!plain.contains(secret));
        let decrypted =
            crypto::decrypt_token(&state.settings.token_encryption_key, &blob).unwrap();
        assert_eq!(decrypted, secret);

        // me reflects flag
        let (status, body) = call(state.clone(), "GET", "/api/auth/me", Some(&cookie), None).await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["has_github_token"], true);
        assert!(!body.contains(secret));

        // delete
        let (status, body) = call(
            state.clone(),
            "DELETE",
            "/api/me/github-token",
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body={body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["has_github_token"], false);
        assert!(!users::user_has_github_token(&state.pool, uid).await.unwrap());

        let (status, body) = call(state.clone(), "GET", "/api/auth/me", Some(&cookie), None).await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["has_github_token"], false);
    }

    #[tokio::test]
    #[serial]
    async fn put_rejects_invalid_github_token() {
        let server = MockServer::start().await;
        mount_rate_limit_unauthorized(&server).await;

        let state = test_state(Some(&server.uri())).await;
        let uid = create_user(&state, "bad_pat_user").await;
        let cookie = auth_cookie(&state, uid, "bad_pat_user");

        let (status, body) = call(
            state.clone(),
            "PUT",
            "/api/me/github-token",
            Some(&cookie),
            Some(serde_json::json!({"token": "ghp_bogus"})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body={body}");
        assert!(body.contains("invalid github token"));
        assert!(!users::user_has_github_token(&state.pool, uid).await.unwrap());
    }
}
