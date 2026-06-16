use super::super::web_server::AppState;
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
    if req.username == state.web_username && req.password == state.web_password {
        Ok(Json(serde_json::json!({
            "success": true,
            "token": state.session_token
        })))
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}
