//! Web Dashboard Authentication handler.
//!
//! Validates login credentials and returns session tokens for dashboard security.

use super::super::web_server::{secret_eq, AppState};
use axum::{
    extract::{ConnectInfo, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::net::SocketAddr;

#[derive(serde::Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// Handle user login, returning a session token
pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(req): Json<LoginRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let ip = peer.ip();
    if !state.login_limiter.allowed(ip) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    // Both comparisons always run: `&&` would skip the password check whenever
    // the username is wrong, making the two failures distinguishable by timing.
    if secret_eq(&req.username, &state.web_username) & secret_eq(&req.password, &state.web_password)
    {
        state.login_limiter.record_success(ip);
        Ok(Json(serde_json::json!({
            "success": true,
            "token": state.session_token
        })))
    } else {
        state.login_limiter.record_failure(ip);
        Err(StatusCode::UNAUTHORIZED)
    }
}
