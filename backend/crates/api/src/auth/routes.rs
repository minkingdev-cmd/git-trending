use super::cookies;
use super::extract::RequireAuth;
use super::passwords;
use super::tokens;
use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use ght_core::refresh as refresh_store;
use ght_core::users;
use serde::{Deserialize, Serialize};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/refresh", post(refresh))
        .route("/logout", post(logout))
        .route("/me", get(me))
}

#[derive(Deserialize)]
pub struct RegisterReq {
    pub username: String,
    pub password: String,
    pub invite_code: String,
}

#[derive(Deserialize)]
pub struct LoginReq {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct AuthResp {
    pub username: String,
}

#[derive(Serialize)]
pub struct MeResp {
    pub user_id: i64,
    pub username: String,
    pub is_admin: bool,
}

fn unauthorized(msg: &str) -> impl IntoResponse {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": msg })),
    )
}

fn issue_cookies(state: &AppState, user_id: i64, username: &str) -> Result<Vec<String>, StatusCode> {
    let jwt = tokens::issue_access(&state.settings.jwt_secret, user_id, username)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(vec![cookies::access_cookie(
        &jwt,
        state.settings.cookie_secure,
    )])
}

async fn register(State(state): State<AppState>, Json(req): Json<RegisterReq>) -> impl IntoResponse {
    if let Err(msg) = passwords::validate_credentials(&req.username, &req.password) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response();
    }
    let username = req.username.trim().to_string();
    let hash = match passwords::hash_password(&req.password) {
        Ok(h) => h,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    match users::register_with_invite(&state.pool, &username, &hash, &req.invite_code).await {
        Ok(user_id) => {
            let refresh = match refresh_store::create_refresh_token(&state.pool, user_id).await {
                Ok(t) => t,
                Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            };
            let mut set_cookies = match issue_cookies(&state, user_id, &username) {
                Ok(c) => c,
                Err(sc) => return sc.into_response(),
            };
            set_cookies.push(cookies::refresh_cookie(
                &refresh,
                state.settings.cookie_secure,
            ));
            let mut resp = (StatusCode::OK, Json(AuthResp { username })).into_response();
            for c in set_cookies {
                if let Ok(val) = c.parse() {
                    resp.headers_mut().append("set-cookie", val);
                }
            }
            resp
        }
        Err(e) => unauthorized(&e.to_string()).into_response(),
    }
}

async fn login(State(state): State<AppState>, Json(req): Json<LoginReq>) -> impl IntoResponse {
    if req.username.trim().is_empty() || req.password.is_empty() {
        return unauthorized("invalid credentials").into_response();
    }
    let user = match users::find_user_by_username(&state.pool, req.username.trim()).await {
        Ok(Some(u)) => u,
        _ => return unauthorized("invalid credentials").into_response(),
    };
    if !passwords::verify_password(&req.password, &user.password_hash) {
        return unauthorized("invalid credentials").into_response();
    }
    let refresh = match refresh_store::create_refresh_token(&state.pool, user.id).await {
        Ok(t) => t,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let mut set_cookies = match issue_cookies(&state, user.id, &user.username) {
        Ok(c) => c,
        Err(sc) => return sc.into_response(),
    };
    set_cookies.push(cookies::refresh_cookie(
        &refresh,
        state.settings.cookie_secure,
    ));
    let mut resp = (
        StatusCode::OK,
        Json(AuthResp {
            username: user.username,
        }),
    )
        .into_response();
    for c in set_cookies {
        if let Ok(val) = c.parse() {
            resp.headers_mut().append("set-cookie", val);
        }
    }
    resp
}

fn read_refresh_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    let header = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    cookies::read_cookie(header, "refresh_token")
}

async fn refresh(State(state): State<AppState>, headers: axum::http::HeaderMap) -> impl IntoResponse {
    let presented = match read_refresh_cookie(&headers) {
        Some(t) => t,
        None => return unauthorized("missing refresh token").into_response(),
    };
    match refresh_store::rotate_refresh_token(&state.pool, &presented).await {
        Ok(user_id) => {
            let username = match users::find_username_by_id(&state.pool, user_id).await {
                Ok(Some(u)) => u,
                _ => return unauthorized("unknown user").into_response(),
            };
            let new_refresh = match refresh_store::create_refresh_token(&state.pool, user_id).await {
                Ok(t) => t,
                Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            };
            let mut set_cookies = match issue_cookies(&state, user_id, &username) {
                Ok(c) => c,
                Err(sc) => return sc.into_response(),
            };
            set_cookies.push(cookies::refresh_cookie(
                &new_refresh,
                state.settings.cookie_secure,
            ));
            let mut resp = (StatusCode::OK, Json(AuthResp { username })).into_response();
            for c in set_cookies {
                if let Ok(val) = c.parse() {
                    resp.headers_mut().append("set-cookie", val);
                }
            }
            resp
        }
        Err(_) => {
            let mut resp = unauthorized("invalid refresh token").into_response();
            for c in cookies::clear_cookies() {
                if let Ok(val) = c.parse() {
                    resp.headers_mut().append("set-cookie", val);
                }
            }
            resp
        }
    }
}

async fn logout(State(state): State<AppState>, headers: axum::http::HeaderMap) -> impl IntoResponse {
    if let Some(t) = read_refresh_cookie(&headers) {
        let _ = refresh_store::delete_refresh_token(&state.pool, &t).await;
    }
    let mut resp = (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response();
    for c in cookies::clear_cookies() {
        if let Ok(val) = c.parse() {
            resp.headers_mut().append("set-cookie", val);
        }
    }
    resp
}

async fn me(State(state): State<AppState>, RequireAuth(claims): RequireAuth) -> impl IntoResponse {
    let is_admin = users::is_bootstrap_admin(&state.pool, claims.sub)
        .await
        .unwrap_or(false);
    Json(MeResp {
        user_id: claims.sub,
        username: claims.username,
        is_admin,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use ght_core::{db, users};
    use serial_test::serial;
    use tower::ServiceExt;

    async fn test_state() -> AppState {
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
        let settings = ght_core::config::Settings::from_map(|k| match k {
            "DATABASE_URL" => Some(url.clone()),
            "JWT_SECRET" => Some("test-secret".into()),
            _ => None,
        })
        .unwrap();
        AppState::new(pool, settings)
    }

    fn json_request(method: &str, uri: &str, body: &str, cookie: Option<&str>) -> Request<Body> {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(c) = cookie {
            req = req.header("cookie", c);
        }
        req.body(Body::from(body.to_string())).unwrap()
    }

    async fn body_string(resp: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn set_cookie(resp: &axum::response::Response, name: &str) -> Option<String> {
        resp.headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find(|c| c.starts_with(&format!("{name}=")))
            .map(|c| c.split(';').next().unwrap().to_string())
    }

    #[tokio::test]
    #[serial]
    async fn register_login_me_flow() {
        let state = test_state().await;
        let invite = users::create_invite(&state.pool, 1).await.unwrap();
        let app = router().with_state(state.clone());

        let resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/register",
                &format!(
                    r#"{{"username":"alice","password":"password123","invite_code":"{invite}"}}"#
                ),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let access = set_cookie(&resp, "access_token").unwrap();
        assert!(set_cookie(&resp, "refresh_token").is_some());

        let resp = app
            .clone()
            .oneshot(json_request("GET", "/me", "", Some(&access)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(body_string(resp).await.contains("alice"));

        let resp = app
            .clone()
            .oneshot(json_request("GET", "/me", "", None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/login",
                r#"{"username":"alice","password":"password123"}"#,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/login",
                r#"{"username":"alice","password":"wrong"}"#,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    #[serial]
    async fn register_with_bad_invite_fails() {
        let state = test_state().await;
        let app = router().with_state(state.clone());
        let resp = app
            .oneshot(json_request(
                "POST",
                "/register",
                r#"{"username":"bob","password":"password123","invite_code":"NOPE"}"#,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    #[serial]
    async fn refresh_rotates_and_reuse_is_rejected() {
        let state = test_state().await;
        let invite = users::create_invite(&state.pool, 1).await.unwrap();
        let app = router().with_state(state.clone());

        let resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/register",
                &format!(
                    r#"{{"username":"carol","password":"password123","invite_code":"{invite}"}}"#
                ),
                None,
            ))
            .await
            .unwrap();
        let refresh = set_cookie(&resp, "refresh_token").unwrap();

        let resp = app
            .clone()
            .oneshot(json_request("POST", "/refresh", "", Some(&refresh)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let new_refresh = set_cookie(&resp, "refresh_token").unwrap();
        assert_ne!(new_refresh, refresh);

        let resp = app
            .clone()
            .oneshot(json_request("POST", "/refresh", "", Some(&refresh)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    #[serial]
    async fn logout_clears_cookies_and_invalidates_refresh() {
        let state = test_state().await;
        let invite = users::create_invite(&state.pool, 1).await.unwrap();
        let app = router().with_state(state.clone());
        let resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/register",
                &format!(
                    r#"{{"username":"dave","password":"password123","invite_code":"{invite}"}}"#
                ),
                None,
            ))
            .await
            .unwrap();
        let refresh = set_cookie(&resp, "refresh_token").unwrap();

        let resp = app
            .clone()
            .oneshot(json_request("POST", "/logout", "", Some(&refresh)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = app
            .clone()
            .oneshot(json_request("POST", "/refresh", "", Some(&refresh)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
