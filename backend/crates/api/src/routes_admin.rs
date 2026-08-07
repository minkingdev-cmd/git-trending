use crate::auth::extract::RequireAuth;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use ght_core::users;
use serde::{Deserialize, Serialize};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/admin/invites", get(list_invites).post(create_invite))
        .route("/api/admin/invites/:code/revoke", post(revoke_invite))
        .route("/api/admin/users", get(list_users))
}

async fn require_admin(state: &AppState, claims: &crate::auth::tokens::Claims) -> Result<(), StatusCode> {
    match users::is_bootstrap_admin(&state.pool, claims.sub).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(StatusCode::FORBIDDEN),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[derive(Serialize)]
struct InviteDto {
    code: String,
    max_uses: i32,
    used_count: i32,
    revoked: bool,
}

#[derive(Deserialize)]
struct CreateInviteReq {
    #[serde(default = "default_uses")]
    max_uses: i32,
}

fn default_uses() -> i32 {
    1
}

#[derive(Serialize)]
struct UserDto {
    id: i64,
    username: String,
    is_admin: bool,
    created_at: String,
}

async fn list_invites(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
) -> impl IntoResponse {
    if let Err(sc) = require_admin(&state, &claims).await {
        return sc.into_response();
    }
    match users::list_invites(&state.pool).await {
        Ok(rows) => Json(
            rows.into_iter()
                .map(|r| InviteDto {
                    code: r.code,
                    max_uses: r.max_uses,
                    used_count: r.used_count,
                    revoked: r.revoked,
                })
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn create_invite(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
    Json(req): Json<CreateInviteReq>,
) -> impl IntoResponse {
    if let Err(sc) = require_admin(&state, &claims).await {
        return sc.into_response();
    }
    if req.max_uses < 1 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "max_uses must be >= 1"})),
        )
            .into_response();
    }
    match users::create_invite(&state.pool, req.max_uses).await {
        Ok(code) => (
            StatusCode::OK,
            Json(serde_json::json!({ "code": code, "max_uses": req.max_uses })),
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn revoke_invite(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
    Path(code): Path<String>,
) -> impl IntoResponse {
    if let Err(sc) = require_admin(&state, &claims).await {
        return sc.into_response();
    }
    match users::revoke_invite(&state.pool, &code).await {
        Ok(true) => Json(serde_json::json!({ "ok": true, "code": code })).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "invite not found"})),
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn list_users(
    State(state): State<AppState>,
    RequireAuth(claims): RequireAuth,
) -> impl IntoResponse {
    if let Err(sc) = require_admin(&state, &claims).await {
        return sc.into_response();
    }
    match users::list_users(&state.pool).await {
        Ok(rows) => Json(
            rows.into_iter()
                .map(|u| UserDto {
                    id: u.id,
                    username: u.username,
                    is_admin: u.is_admin,
                    created_at: u.created_at.to_rfc3339(),
                })
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use crate::auth::{passwords, tokens};
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
        AppState { pool, settings }
    }

    async fn post_json(
        state: AppState,
        uri: &str,
        body: &str,
        cookie: Option<&str>,
    ) -> (StatusCode, String) {
        let mut b = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(c) = cookie {
            b = b.header("cookie", c);
        }
        let resp = crate::build_router(state)
            .oneshot(b.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    async fn get(state: AppState, uri: &str, cookie: Option<&str>) -> (StatusCode, String) {
        let mut b = Request::builder().method("GET").uri(uri);
        if let Some(c) = cookie {
            b = b.header("cookie", c);
        }
        let resp = crate::build_router(state)
            .oneshot(b.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    #[tokio::test]
    #[serial]
    async fn bootstrap_admin_can_manage_invites_regular_user_forbidden() {
        let state = test_state().await;
        let hash = passwords::hash_password("password123").unwrap();
        let admin_id = users::create_user(&state.pool, "admin", &hash, None)
            .await
            .unwrap();
        let invite = users::create_invite(&state.pool, 1).await.unwrap();
        users::register_with_invite(&state.pool, "regular", &hash, &invite)
            .await
            .unwrap();

        let admin_cookie = format!(
            "access_token={}",
            tokens::issue_access(&state.settings.jwt_secret, admin_id, "admin").unwrap()
        );
        let regular = users::find_user_by_username(&state.pool, "regular")
            .await
            .unwrap()
            .unwrap();
        let user_cookie = format!(
            "access_token={}",
            tokens::issue_access(&state.settings.jwt_secret, regular.id, "regular").unwrap()
        );

        let (status, body) = post_json(
            state.clone(),
            "/api/admin/invites",
            r#"{"max_uses":3}"#,
            Some(&admin_cookie),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("code"));

        let (status, _) = get(state.clone(), "/api/admin/invites", Some(&user_cookie)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, body) = get(state.clone(), "/api/admin/users", Some(&admin_cookie)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("admin") && body.contains("regular"));
    }
}
