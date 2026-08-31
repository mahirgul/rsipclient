//! Web Dashboard Authentication handler.
//!
//! Validates login credentials and returns session tokens for dashboard security.

use super::super::web_server::{secret_eq, AppState};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};

#[derive(serde::Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// Handle user login, returning a session token
pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    // Both comparisons always run: `&&` would skip the password check whenever
    // the username is wrong, making the two failures distinguishable by timing.
    if secret_eq(&req.username, &state.web_username) & secret_eq(&req.password, &state.web_password)
    {
        Ok(Json(serde_json::json!({
            "success": true,
            "token": state.session_token
        })))
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}
