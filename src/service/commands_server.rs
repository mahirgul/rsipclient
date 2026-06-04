//! REST Command execution service for remote controlling the SIP service.

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use base64::Engine;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::ManagedClient;
use crate::ipc::{Request, Response};

#[derive(Clone)]
pub struct CommandsServerState {
    pub clients: Arc<Mutex<HashMap<String, ManagedClient>>>,
    pub global_shutdown: Arc<Mutex<bool>>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub fallback_web_username: String,
    pub fallback_web_password: String,
}

/// Verify Basic Authentication
fn verify_auth(headers: &HeaderMap, state: &CommandsServerState) -> Result<(), StatusCode> {
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !auth_header.starts_with("Basic ") {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let encoded = auth_header.strip_prefix("Basic ").unwrap();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let decoded_str = String::from_utf8(decoded).map_err(|_| StatusCode::BAD_REQUEST)?;

    let parts: Vec<&str> = decoded_str.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let username = parts[0];
    let password = parts[1];

    let expected_username = state
        .username
        .as_deref()
        .unwrap_or(&state.fallback_web_username);
    let expected_password = state
        .password
        .as_deref()
        .unwrap_or(&state.fallback_web_password);

    if username == expected_username && password == expected_password {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// Helper to execute an IPC-style command against the internal handlers
async fn execute_cmd(req: Request, state: &CommandsServerState) -> Response {
    if req.cmd == "shutdown" {
        log::info!("Shutdown command received via REST API.");
        *state.global_shutdown.lock().await = true;
        return Response::ok("Service is shutting down");
    }

    // should_register is handled by process_command → handle_register/handle_unregister
    // No need to set it here redundantly

    let cls = {
        let guard = state.clients.lock().await;
        guard.clone()
    };
    super::handlers::process_command(&req, &cls).await
}

// ── GET /api/cmd/status ──
async fn handle_get_status(
    State(state): State<CommandsServerState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_auth(&headers, &state)?;
    let req = Request::new("status");
    let resp = execute_cmd(req, &state).await;
    Ok(Json(resp))
}

// ── POST /api/cmd ──
async fn handle_post_cmd(
    State(state): State<CommandsServerState>,
    headers: HeaderMap,
    Json(req): Json<Request>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_auth(&headers, &state)?;
    let resp = execute_cmd(req, &state).await;
    Ok(Json(resp))
}

// ── POST /api/cmd/register ──
#[derive(serde::Deserialize)]
struct TargetAccountPayload {
    account: String,
}

async fn handle_post_register(
    State(state): State<CommandsServerState>,
    headers: HeaderMap,
    Json(payload): Json<TargetAccountPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_auth(&headers, &state)?;
    let req = Request::with_account("register", &payload.account);
    let resp = execute_cmd(req, &state).await;
    Ok(Json(resp))
}

// ── POST /api/cmd/unregister ──
async fn handle_post_unregister(
    State(state): State<CommandsServerState>,
    headers: HeaderMap,
    Json(payload): Json<TargetAccountPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_auth(&headers, &state)?;
    let req = Request::with_account("unregister", &payload.account);
    let resp = execute_cmd(req, &state).await;
    Ok(Json(resp))
}

// ── POST /api/cmd/call ──
#[derive(serde::Deserialize)]
struct CallPayload {
    account: String,
    target: String,
}

async fn handle_post_call(
    State(state): State<CommandsServerState>,
    headers: HeaderMap,
    Json(payload): Json<CallPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_auth(&headers, &state)?;
    let req = Request::with_target("call", &payload.account, &payload.target);
    let resp = execute_cmd(req, &state).await;
    Ok(Json(resp))
}

// ── POST /api/cmd/hangup ──
async fn handle_post_hangup(
    State(state): State<CommandsServerState>,
    headers: HeaderMap,
    Json(payload): Json<TargetAccountPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_auth(&headers, &state)?;
    let req = Request::with_account("hangup", &payload.account);
    let resp = execute_cmd(req, &state).await;
    Ok(Json(resp))
}

// ── POST /api/cmd/cancel ──
async fn handle_post_cancel(
    State(state): State<CommandsServerState>,
    headers: HeaderMap,
    Json(payload): Json<TargetAccountPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_auth(&headers, &state)?;
    let req = Request::with_account("cancel", &payload.account);
    let resp = execute_cmd(req, &state).await;
    Ok(Json(resp))
}

// ── POST /api/cmd/play ──
#[derive(serde::Deserialize)]
struct PlayPayload {
    account: String,
    file: String,
}

async fn handle_post_play(
    State(state): State<CommandsServerState>,
    headers: HeaderMap,
    Json(payload): Json<PlayPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_auth(&headers, &state)?;
    let req = Request::with_target("play", &payload.account, &payload.file);
    let resp = execute_cmd(req, &state).await;
    Ok(Json(resp))
}

// ── POST /api/cmd/shutdown ──
async fn handle_post_shutdown(
    State(state): State<CommandsServerState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_auth(&headers, &state)?;
    let req = Request::new("shutdown");
    let resp = execute_cmd(req, &state).await;
    Ok(Json(resp))
}

// ── POST /api/cmd/hold ──
async fn handle_post_hold(
    State(state): State<CommandsServerState>,
    headers: HeaderMap,
    Json(payload): Json<TargetAccountPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_auth(&headers, &state)?;
    let req = Request::with_account("hold", &payload.account);
    let resp = execute_cmd(req, &state).await;
    Ok(Json(resp))
}

// ── POST /api/cmd/resume ──
async fn handle_post_resume(
    State(state): State<CommandsServerState>,
    headers: HeaderMap,
    Json(payload): Json<TargetAccountPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_auth(&headers, &state)?;
    let req = Request::with_account("resume", &payload.account);
    let resp = execute_cmd(req, &state).await;
    Ok(Json(resp))
}

// ── POST /api/cmd/transfer ──
async fn handle_post_transfer(
    State(state): State<CommandsServerState>,
    headers: HeaderMap,
    Json(payload): Json<CallPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_auth(&headers, &state)?;
    let req = Request::with_target("transfer", &payload.account, &payload.target);
    let resp = execute_cmd(req, &state).await;
    Ok(Json(resp))
}

// ── POST /api/cmd/dtmf ──
#[derive(serde::Deserialize)]
struct DtmfPayload {
    account: String,
    digits: String,
}

async fn handle_post_dtmf(
    State(state): State<CommandsServerState>,
    headers: HeaderMap,
    Json(payload): Json<DtmfPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_auth(&headers, &state)?;
    let req = Request::with_target("dtmf", &payload.account, &payload.digits);
    let resp = execute_cmd(req, &state).await;
    Ok(Json(resp))
}

/// Start the REST Commands server on the specified port
pub async fn start_commands_server(state: CommandsServerState, port: u16) {
    let app = Router::new()
        .route("/api/cmd", post(handle_post_cmd))
        .route("/api/cmd/register", post(handle_post_register))
        .route("/api/cmd/unregister", post(handle_post_unregister))
        .route("/api/cmd/call", post(handle_post_call))
        .route("/api/cmd/hangup", post(handle_post_hangup))
        .route("/api/cmd/cancel", post(handle_post_cancel))
        .route("/api/cmd/hold", post(handle_post_hold))
        .route("/api/cmd/resume", post(handle_post_resume))
        .route("/api/cmd/transfer", post(handle_post_transfer))
        .route("/api/cmd/dtmf", post(handle_post_dtmf))
        .route("/api/cmd/play", post(handle_post_play))
        .route(
            "/api/cmd/status",
            get(handle_get_status).post(handle_get_status),
        )
        .route("/api/cmd/shutdown", post(handle_post_shutdown))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    log::info!("Starting REST command service on http://{}", addr);

    match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => {
            if let Err(e) = axum::serve(listener, app).await {
                log::error!("REST command server error: {}", e);
            }
        }
        Err(e) => {
            log::error!("Failed to bind REST command server to port {}: {}", port, e);
        }
    }
}
