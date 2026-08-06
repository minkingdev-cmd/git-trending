use super::cookies;
use super::tokens::{self, Claims};
use crate::state::AppState;
use axum::async_trait;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

pub struct RequireAuth(pub Claims);

pub struct AuthError;

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response()
    }
}

#[async_trait]
impl FromRequestParts<AppState> for RequireAuth {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(axum::http::header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .ok_or(AuthError)?;
        let token = cookies::read_cookie(header, "access_token").ok_or(AuthError)?;
        let claims =
            tokens::verify_access(&state.settings.jwt_secret, &token).map_err(|_| AuthError)?;
        Ok(RequireAuth(claims))
    }
}
