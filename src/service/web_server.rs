//! Web Server module - provides the REST API and serves the embedded Dashboard UI

use super::{create_managed_client, ManagedClient};
use crate::config::{Account, Config};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Json},
    routing::{delete, get, post, put},
    Router,
};
use futures_util::{sink::SinkExt, stream::StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use sysinfo::System;
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
}

#[derive(serde::Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(serde::Serialize)]
struct StatusResponse {
    uptime_secs: u64,
    memory_bytes: u64,
    cpu_percent: f32,
    os_name: String,
    total_accounts: usize,
    registered_accounts: usize,
    active_calls: usize,
    accounts: Vec<AccountStatus>,
}

#[derive(serde::Serialize)]
struct AccountStatus {
    name: String,
    username: String,
    domain: String,
    server: String,
    sip_port: u16,
    registered: bool,
    in_call: bool,
    call_id: Option<String>,
    codec: String,
    codec_rate: u32,
}

/// Helper to verify Authorization header token
fn verify_token(headers: &HeaderMap, state: &AppState) -> Result<(), StatusCode> {
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

/// Handle user login, returning a session token
async fn login(
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

/// Get call status, registrations, and process diagnostics
async fn get_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    let cls = state.clients.lock().await;

    let mut total_accounts = 0;
    let mut registered_accounts = 0;
    let mut active_calls = 0;
    let mut accounts = vec![];

    for (name, mc) in cls.iter() {
        total_accounts += 1;
        let client_lock = mc.client.lock().await;
        let is_registered = *client_lock.registered.lock().await;
        let is_in_call = client_lock.in_call;

        if is_registered {
            registered_accounts += 1;
        }
        if is_in_call {
            active_calls += 1;
        }

        let codec_str = mc
            .account
            .codec
            .clone()
            .unwrap_or_else(|| "pcmu".to_string());
        let codec_rate = mc.codec.clock_rate();

        accounts.push(AccountStatus {
            name: name.clone(),
            username: client_lock.username.clone(),
            domain: client_lock.domain.clone(),
            server: client_lock.server_addr.to_string(),
            sip_port: client_lock.local_addr.port(),
            registered: is_registered,
            in_call: is_in_call,
            call_id: client_lock.call_id.clone(),
            codec: codec_str,
            codec_rate,
        });
    }

    // Process system and resource info
    let mut sys = System::new_all();
    sys.refresh_all();

    let pid = sysinfo::get_current_pid().ok();
    let mut memory_bytes = 0;
    let mut cpu_percent = 0.0;

    if let Some(pid) = pid {
        if let Some(proc) = sys.process(pid) {
            // proc.memory() returns memory in bytes in modern sysinfo (version 0.30+)
            memory_bytes = proc.memory();
            cpu_percent = proc.cpu_usage();
        }
    }

    let uptime_secs = state.start_time.elapsed().as_secs();
    let os_name = format!(
        "{} {}",
        System::name().unwrap_or_else(|| "Unknown OS".to_string()),
        System::os_version().unwrap_or_default()
    );

    Ok(Json(StatusResponse {
        uptime_secs,
        memory_bytes,
        cpu_percent,
        os_name,
        total_accounts,
        registered_accounts,
        active_calls,
        accounts,
    }))
}

/// Retrieve the raw list of configured accounts
async fn get_accounts(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    let cfg = Config::load(&state.config_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(cfg.accounts))
}

/// Dynamically add a new account, spawn its listener, and save to TOML
async fn add_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(new_acc): Json<Account>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    let mut cls = state.clients.lock().await;
    if cls.contains_key(&new_acc.name) {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Load config, append account, save config
    let mut cfg =
        Config::load(&state.config_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    cfg.accounts.push(new_acc.clone());
    cfg.save(&state.config_path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Create managed client and insert
    let mc = create_managed_client(&new_acc).await.map_err(|e| {
        log::error!("Failed to create dynamic client: {}", e);
        StatusCode::BAD_REQUEST
    })?;

    // Spawn call watcher if auto-answer
    if mc.account.auto_answer.unwrap_or(false) {
        let client = mc.client.clone();
        let codec = mc.codec;
        let account = mc.account.clone();
        let shutdown = state.global_shutdown.clone();
        let active = mc.active.clone();
        let audio_tx = mc.audio_tx.clone();
        let account_name = new_acc.name.clone();
        tokio::spawn(async move {
            super::incoming_call_watcher(
                account_name,
                client,
                codec,
                account,
                shutdown,
                active,
                audio_tx,
            )
            .await;
        });
    }

    cls.insert(new_acc.name.clone(), mc);
    Ok(Json(serde_json::json!({ "success": true })))
}

/// Dynamically edit an existing account, reload it, and save to TOML
async fn edit_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(updated_acc): Json<Account>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    let mut cls = state.clients.lock().await;
    let old_mc = cls.get(&name).ok_or(StatusCode::NOT_FOUND)?;

    // Stop watcher task of old client
    *old_mc.active.lock().await = false;

    // Load config, update, save config
    let mut cfg =
        Config::load(&state.config_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(idx) = cfg.accounts.iter().position(|a| a.name == name) {
        cfg.accounts[idx] = updated_acc.clone();
    } else {
        return Err(StatusCode::NOT_FOUND);
    }
    cfg.save(&state.config_path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Create new managed client
    let mc = create_managed_client(&updated_acc).await.map_err(|e| {
        log::error!("Failed to create dynamic client: {}", e);
        StatusCode::BAD_REQUEST
    })?;

    // Spawn call watcher if auto-answer
    if mc.account.auto_answer.unwrap_or(false) {
        let client = mc.client.clone();
        let codec = mc.codec;
        let account = mc.account.clone();
        let shutdown = state.global_shutdown.clone();
        let active = mc.active.clone();
        let audio_tx = mc.audio_tx.clone();
        let account_name = updated_acc.name.clone();
        tokio::spawn(async move {
            super::incoming_call_watcher(
                account_name,
                client,
                codec,
                account,
                shutdown,
                active,
                audio_tx,
            )
            .await;
        });
    }

    // Remove old, insert new (handles renaming)
    cls.remove(&name);
    cls.insert(updated_acc.name.clone(), mc);

    Ok(Json(serde_json::json!({ "success": true })))
}

/// Dynamically remove an account, stop its watcher, and save to TOML
async fn delete_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    let mut cls = state.clients.lock().await;
    let old_mc = cls.remove(&name).ok_or(StatusCode::NOT_FOUND)?;

    // Stop watcher task
    *old_mc.active.lock().await = false;

    // Load config, remove, save config
    let mut cfg =
        Config::load(&state.config_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    cfg.accounts.retain(|a| a.name != name);
    cfg.save(&state.config_path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({ "success": true })))
}

/// Force trigger a REGISTER request for a specific account
async fn register_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    let cls = state.clients.lock().await;
    let mc = cls.get(&name).ok_or(StatusCode::NOT_FOUND)?;
    *mc.should_register.lock().await = true;
    let client = mc.client.lock().await;

    match client.register().await {
        Ok(true) => Ok(Json(
            serde_json::json!({ "success": true, "msg": "Registered successfully" }),
        )),
        Ok(false) => Ok(Json(
            serde_json::json!({ "success": false, "msg": "Registration failed" }),
        )),
        Err(e) => Ok(Json(
            serde_json::json!({ "success": false, "msg": format!("Error: {}", e) }),
        )),
    }
}

/// Force trigger an UNREGISTER (Expires: 0) request for a specific account
async fn unregister_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    let cls = state.clients.lock().await;
    let mc = cls.get(&name).ok_or(StatusCode::NOT_FOUND)?;
    *mc.should_register.lock().await = false;
    let client = mc.client.lock().await;

    match client.unregister().await {
        Ok(true) => Ok(Json(
            serde_json::json!({ "success": true, "msg": "Unregistered successfully" }),
        )),
        Ok(false) => Ok(Json(
            serde_json::json!({ "success": false, "msg": "Unregistration failed" }),
        )),
        Err(e) => Ok(Json(
            serde_json::json!({ "success": false, "msg": format!("Error: {}", e) }),
        )),
    }
}

/// Fetch recent logs captured in memory
async fn get_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    let logs = super::logger::get_recent_logs();
    Ok(Json(logs))
}

/// Fetch the raw Config struct
async fn get_config(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;
    let cfg = Config::load(&state.config_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(cfg))
}

/// Update the global config file and dynamically reload clients
async fn put_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(new_config): Json<Config>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    // Save config to file
    new_config.save(&state.config_path).map_err(|e| {
        log::error!(
            "Failed to save config to path '{}': {}",
            state.config_path,
            e
        );
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Perform dynamic reload of all clients
    if let Err(e) = reload_all_clients(&state.clients, &new_config, &state.global_shutdown).await {
        log::error!("Failed to reload clients dynamically: {}", e);
        return Err(StatusCode::BAD_REQUEST);
    }

    Ok(Json(
        serde_json::json!({ "success": true, "msg": "Configuration updated and reloaded successfully" }),
    ))
}

async fn reload_all_clients(
    clients: &Arc<Mutex<HashMap<String, ManagedClient>>>,
    config: &Config,
    global_shutdown: &Arc<Mutex<bool>>,
) -> Result<(), anyhow::Error> {
    let mut cls = clients.lock().await;

    // Stop all active call and registration watchers
    for mc in cls.values() {
        *mc.active.lock().await = false;
    }
    cls.clear();

    // Create new clients from config
    for account in &config.accounts {
        let mc = create_managed_client(account).await?;
        let client = mc.client.clone();
        let codec = mc.codec;
        let acc = mc.account.clone();
        let active = mc.active.clone();
        let should_register = mc.should_register.clone();
        let register_expiry = mc.account.register_expiry.unwrap_or(3600);
        let retry_interval = mc.account.register_retry_interval.unwrap_or(30);
        let account_name = account.name.clone();

        // Spawn call watcher if auto answer
        if acc.auto_answer.unwrap_or(false) {
            let shutdown = global_shutdown.clone();
            let name = account_name.clone();
            let c_clone = client.clone();
            let a_clone = active.clone();
            let audio_tx = mc.audio_tx.clone();
            tokio::spawn(async move {
                super::incoming_call_watcher(
                    name, c_clone, codec, acc, shutdown, a_clone, audio_tx,
                )
                .await;
            });
        }

        // Spawn registration watcher
        let shutdown = global_shutdown.clone();
        let name = account_name.clone();
        let c_clone = client.clone();
        let a_clone = active.clone();
        tokio::spawn(async move {
            super::registration_watcher(
                name,
                c_clone,
                a_clone,
                should_register,
                register_expiry,
                retry_interval,
                shutdown,
            )
            .await;
        });

        cls.insert(account_name, mc);
    }

    Ok(())
}

async fn audio_ws_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
    ws: WebSocketUpgrade,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, StatusCode> {
    let token = params.get("token").cloned();
    if token.as_deref() != Some(&state.session_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    {
        let cls = state.clients.lock().await;
        if !cls.contains_key(&name) {
            return Err(StatusCode::NOT_FOUND);
        }
    }

    Ok(ws.on_upgrade(move |socket| handle_audio_ws(socket, state, name)))
}

async fn handle_audio_ws(socket: WebSocket, state: AppState, account_name: String) {
    let clients = state.clients.lock().await;
    let mc = match clients.get(&account_name) {
        Some(mc) => mc,
        None => return,
    };

    let mut audio_rx = mc.audio_tx.subscribe();
    let client = mc.client.clone();
    let codec = mc.codec;
    drop(clients);

    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Loop 1: Send incoming caller audio to browser (binary frames of Vec<i16>)
    let mut send_task = tokio::spawn(async move {
        while let Ok(samples) = audio_rx.recv().await {
            let mut bytes = Vec::with_capacity(samples.len() * 2);
            for sample in samples {
                bytes.extend_from_slice(&sample.to_le_bytes());
            }
            if ws_sender.send(Message::Binary(bytes)).await.is_err() {
                break;
            }
        }
    });

    // Loop 2: Receive browser microphone audio (binary frames of i16) and send it as RTP to caller
    let client_clone = client.clone();
    let mut recv_task = tokio::spawn(async move {
        let mut seq = 0;
        let mut timestamp = 0;

        while let Some(Ok(msg)) = ws_receiver.next().await {
            if let Message::Binary(bytes) = msg {
                let mut samples = Vec::with_capacity(bytes.len() / 2);
                for chunk in bytes.chunks_exact(2) {
                    let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
                    samples.push(sample);
                }

                let cg = client_clone.lock().await;
                if cg.in_call {
                    if let (Some(ref rtp_rec), Some(target)) =
                        (&cg.rtp_receiver, cg.remote_rtp_addr)
                    {
                        let _ = rtp_rec
                            .send_audio_samples(&samples, target, codec, &mut seq, &mut timestamp)
                            .await;
                    }
                }
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => { recv_task.abort(); }
        _ = &mut recv_task => { send_task.abort(); }
    }
}

/// Launch the Axum HTTP server
pub async fn start_web_server(state: AppState, port: u16) {
    let app = Router::new()
        .route("/", get(index))
        .route("/api/login", post(login))
        .route("/api/status", get(get_status))
        .route("/api/accounts", get(get_accounts))
        .route("/api/accounts", post(add_account))
        .route("/api/accounts/:name", put(edit_account))
        .route("/api/accounts/:name", delete(delete_account))
        .route("/api/accounts/:name/register", post(register_account))
        .route("/api/accounts/:name/unregister", post(unregister_account))
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
