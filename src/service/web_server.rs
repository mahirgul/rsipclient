//! Web Server module - provides the REST API and serves the embedded Dashboard UI

use super::web_handlers::*;
use super::ManagedClient;
use axum::{
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse},
    routing::{delete, get, post, put},
    Router,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AppState {
    pub clients: Arc<Mutex<HashMap<String, ManagedClient>>>,
    pub global_shutdown: Arc<Mutex<bool>>,
    pub config_path: String,
    pub web_username: String,
    pub web_password: String,
    pub session_token: String,
    pub start_time: std::time::Instant,
    pub sys: Arc<Mutex<sysinfo::System>>,
}

#[derive(serde::Serialize)]
pub struct StatusResponse {
    pub uptime_secs: u64,
    pub memory_bytes: u64,
    pub cpu_percent: f32,
    pub os_name: String,
    pub total_accounts: usize,
    pub registered_accounts: usize,
    pub active_calls: usize,
    pub accounts: Vec<AccountStatus>,
}

#[derive(serde::Serialize)]
pub struct AccountStatus {
    pub name: String,
    pub username: String,
    pub domain: String,
    pub server: String,
    pub sip_port: u16,
    pub registered: bool,
    pub in_call: bool,
    pub held: bool,
    pub call_id: Option<String>,
    pub codec: String,
    pub codec_rate: u32,
}

/// Helper to verify Authorization header token
pub fn verify_token(headers: &HeaderMap, state: &AppState) -> Result<(), StatusCode> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));

    if let Some(tok) = token {
        if tok == state.session_token {
            return Ok(());
        }
    }

    Err(StatusCode::UNAUTHORIZED)
}

/// Serve the single-page HTML dashboard
async fn index() -> impl IntoResponse {
    Html(include_str!("web/index.html"))
}

async fn style_css() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/css")],
        include_str!("web/style.css"),
    )
}

async fn auth_js() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/javascript")],
        include_str!("web/auth.js"),
    )
}

async fn audio_js() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/javascript")],
        include_str!("web/audio.js"),
    )
}

async fn config_js() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/javascript")],
        include_str!("web/config.js"),
    )
}

async fn app_js() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/javascript")],
        include_str!("web/app.js"),
    )
}

async fn favicon() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "image/x-icon")],
        include_bytes!("web/favicon.ico").as_slice(),
    )
}

/// Launch the Axum HTTP server
pub async fn start_web_server(state: AppState, port: u16) {
    let app = Router::new()
        .route("/", get(index))
        .route("/style.css", get(style_css))
        .route("/auth.js", get(auth_js))
        .route("/audio.js", get(audio_js))
        .route("/config.js", get(config_js))
        .route("/app.js", get(app_js))
        .route("/favicon.ico", get(favicon))
        .route("/api/login", post(login))
        .route("/api/status", get(get_status))
        .route("/api/accounts", get(get_accounts))
        .route("/api/accounts", post(add_account))
        .route("/api/accounts/:name", put(edit_account))
        .route("/api/accounts/:name", delete(delete_account))
        .route("/api/accounts/:name/register", post(register_account))
        .route("/api/accounts/:name/unregister", post(unregister_account))
        .route("/api/accounts/:name/call", post(call_account))
        .route("/api/accounts/:name/hangup", post(hangup_account))
        .route("/api/accounts/:name/hold", post(hold_account))
        .route("/api/accounts/:name/resume", post(resume_account))
        .route("/api/accounts/:name/transfer", post(transfer_account))
        .route("/api/accounts/:name/dtmf", post(dtmf_account))
        .route("/api/accounts/:name/play", post(play_account))
        .route("/api/config", get(get_config))
        .route("/api/config", put(put_config))
        .route("/api/logs", get(get_logs))
        .route("/api/accounts/:name/audio-ws", get(audio_ws_handler))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    log::info!("Starting dashboard web server on http://{}", addr);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            log::error!("Failed to bind web server to port {}: {}", port, e);
            return;
        }
    };

    if let Err(e) = axum::serve(listener, app).await {
        log::error!("Axum web server error: {}", e);
    }
}
