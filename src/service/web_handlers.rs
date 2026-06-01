//! Web handlers for Axum router API endpoints

use super::web_server::{verify_token, AccountStatus, AppState, StatusResponse};
use super::{create_managed_client, ManagedClient};
use crate::config::{Account, Config};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use futures_util::{sink::SinkExt, stream::StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use sysinfo::System;
use tokio::sync::Mutex;

#[derive(serde::Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(serde::Deserialize)]
pub struct CallRequest {
    pub target: String,
}

#[derive(serde::Deserialize)]
pub struct DtmfRequest {
    pub digits: String,
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

/// Get call status, registrations, and process diagnostics
pub async fn get_status(
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

        let is_held = client_lock.held;

        accounts.push(AccountStatus {
            name: name.clone(),
            username: client_lock.username.clone(),
            domain: client_lock.domain.clone(),
            server: client_lock.server_addr.to_string(),
            sip_port: client_lock.local_addr.port(),
            registered: is_registered,
            in_call: is_in_call,
            held: is_held,
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
pub async fn get_accounts(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    let cfg = Config::load(&state.config_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(cfg.accounts))
}

/// Dynamically add a new account, spawn its listener, and save to TOML
pub async fn add_account(
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

    // Spawn registration watcher
    {
        let client = mc.client.clone();
        let active = mc.active.clone();
        let should_register = mc.should_register.clone();
        let register_expiry = mc.account.register_expiry.unwrap_or(3600);
        let retry_interval = mc.account.register_retry_interval.unwrap_or(30);
        let shutdown = state.global_shutdown.clone();
        let account_name = new_acc.name.clone();
        tokio::spawn(async move {
            super::registration_watcher(
                account_name,
                client,
                active,
                should_register,
                register_expiry,
                retry_interval,
                shutdown,
            )
            .await;
        });
    }

    cls.insert(new_acc.name.clone(), mc);
    Ok(Json(serde_json::json!({ "success": true })))
}

/// Dynamically edit an existing account, reload it, and save to TOML
pub async fn edit_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(updated_acc): Json<Account>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    let mut cls = state.clients.lock().await;

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

    // If the old client was successfully running/initialized, stop and drop it
    if let Some(old_mc) = cls.remove(&name) {
        // Stop watcher task of old client
        *old_mc.active.lock().await = false;

        // Wait for the background watchers to drop their references to the old client, releasing the socket
        let old_client_arc = old_mc.client.clone();
        drop(old_mc);

        let mut retries = 0;
        while Arc::strong_count(&old_client_arc) > 1 && retries < 40 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            retries += 1;
        }
        drop(old_client_arc);
    }

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

    // Spawn registration watcher
    {
        let client = mc.client.clone();
        let active = mc.active.clone();
        let should_register = mc.should_register.clone();
        let register_expiry = mc.account.register_expiry.unwrap_or(3600);
        let retry_interval = mc.account.register_retry_interval.unwrap_or(30);
        let shutdown = state.global_shutdown.clone();
        let account_name = updated_acc.name.clone();
        tokio::spawn(async move {
            super::registration_watcher(
                account_name,
                client,
                active,
                should_register,
                register_expiry,
                retry_interval,
                shutdown,
            )
            .await;
        });
    }

    // Insert new (handles renaming)
    cls.insert(updated_acc.name.clone(), mc);

    Ok(Json(serde_json::json!({ "success": true })))
}

/// Dynamically remove an account, stop its watcher, and save to TOML
pub async fn delete_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    let mut cls = state.clients.lock().await;

    // Load config, remove, save config
    let mut cfg =
        Config::load(&state.config_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Check if the account exists in the config before trying to delete it
    let exists_in_config = cfg.accounts.iter().any(|a| a.name == name);
    if !exists_in_config {
        return Err(StatusCode::NOT_FOUND);
    }

    cfg.accounts.retain(|a| a.name != name);
    cfg.save(&state.config_path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Stop watcher task if the client was running
    if let Some(old_mc) = cls.remove(&name) {
        *old_mc.active.lock().await = false;
    }

    Ok(Json(serde_json::json!({ "success": true })))
}

/// Force trigger a REGISTER request for a specific account
pub async fn register_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    // Set should_register flag and clone client Arc, then drop HashMap lock before I/O
    let client_arc = {
        let cls = state.clients.lock().await;
        let mc = cls.get(&name).ok_or(StatusCode::NOT_FOUND)?;
        *mc.should_register.lock().await = true;
        mc.client.clone()
    };
    let client = client_arc.lock().await;

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
pub async fn unregister_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    // Clone client Arc, then drop HashMap lock before I/O
    let client_arc = {
        let cls = state.clients.lock().await;
        let mc = cls.get(&name).ok_or(StatusCode::NOT_FOUND)?;
        *mc.should_register.lock().await = false;
        mc.client.clone()
    };
    let client = client_arc.lock().await;

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

pub async fn call_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<CallRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    // Clone everything we need, then drop HashMap lock before I/O
    let (client_arc, codec, audio_tx) = {
        let cls = state.clients.lock().await;
        let mc = cls.get(&name).ok_or(StatusCode::NOT_FOUND)?;
        (mc.client.clone(), mc.codec, mc.audio_tx.clone())
    };
    let mut client = client_arc.lock().await;

    match client.invite(&payload.target).await {
        Ok(true) => {
            if let Some(ref rx) = client.rtp_receiver {
                rx.start(codec, Some(audio_tx));
            }
            Ok(Json(
                serde_json::json!({ "success": true, "msg": "Call established" }),
            ))
        }
        Ok(false) => Ok(Json(
            serde_json::json!({ "success": false, "msg": "Call failed" }),
        )),
        Err(e) => Ok(Json(
            serde_json::json!({ "success": false, "msg": format!("Error: {}", e) }),
        )),
    }
}

pub async fn hangup_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;
    let client_arc = {
        let cls = state.clients.lock().await;
        let mc = cls.get(&name).ok_or(StatusCode::NOT_FOUND)?;
        mc.client.clone()
    };
    let mut client = client_arc.lock().await;
    match client.bye().await {
        Ok(true) => Ok(Json(
            serde_json::json!({ "success": true, "msg": "Call ended" }),
        )),
        Ok(false) => Ok(Json(
            serde_json::json!({ "success": false, "msg": "No active call" }),
        )),
        Err(e) => Ok(Json(
            serde_json::json!({ "success": false, "msg": format!("Error: {}", e) }),
        )),
    }
}

pub async fn hold_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;
    let client_arc = {
        let cls = state.clients.lock().await;
        let mc = cls.get(&name).ok_or(StatusCode::NOT_FOUND)?;
        mc.client.clone()
    };
    let mut client = client_arc.lock().await;
    match client.hold().await {
        Ok(true) => Ok(Json(
            serde_json::json!({ "success": true, "msg": "Call put on hold" }),
        )),
        Ok(false) => Ok(Json(
            serde_json::json!({ "success": false, "msg": "Hold failed or no active call" }),
        )),
        Err(e) => Ok(Json(
            serde_json::json!({ "success": false, "msg": format!("Error: {}", e) }),
        )),
    }
}

pub async fn resume_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;
    let client_arc = {
        let cls = state.clients.lock().await;
        let mc = cls.get(&name).ok_or(StatusCode::NOT_FOUND)?;
        mc.client.clone()
    };
    let mut client = client_arc.lock().await;
    match client.resume().await {
        Ok(true) => Ok(Json(
            serde_json::json!({ "success": true, "msg": "Call resumed" }),
        )),
        Ok(false) => Ok(Json(
            serde_json::json!({ "success": false, "msg": "Resume failed or no active call" }),
        )),
        Err(e) => Ok(Json(
            serde_json::json!({ "success": false, "msg": format!("Error: {}", e) }),
        )),
    }
}

pub async fn transfer_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<CallRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;
    let client_arc = {
        let cls = state.clients.lock().await;
        let mc = cls.get(&name).ok_or(StatusCode::NOT_FOUND)?;
        mc.client.clone()
    };
    let mut client = client_arc.lock().await;
    match client.transfer(&payload.target).await {
        Ok(true) => Ok(Json(
            serde_json::json!({ "success": true, "msg": "Transfer initiated" }),
        )),
        Ok(false) => Ok(Json(
            serde_json::json!({ "success": false, "msg": "Transfer failed or no active call" }),
        )),
        Err(e) => Ok(Json(
            serde_json::json!({ "success": false, "msg": format!("Error: {}", e) }),
        )),
    }
}

pub async fn dtmf_account(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<DtmfRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;
    let client_arc = {
        let cls = state.clients.lock().await;
        let mc = cls.get(&name).ok_or(StatusCode::NOT_FOUND)?;
        mc.client.clone()
    };
    let mut client = client_arc.lock().await;
    match client.send_dtmf(&payload.digits).await {
        Ok(true) => Ok(Json(
            serde_json::json!({ "success": true, "msg": format!("Sent DTMF: {}", payload.digits) }),
        )),
        Ok(false) => Ok(Json(
            serde_json::json!({ "success": false, "msg": "DTMF failed or no active call" }),
        )),
        Err(e) => Ok(Json(
            serde_json::json!({ "success": false, "msg": format!("Error: {}", e) }),
        )),
    }
}

/// Fetch recent logs captured in memory
pub async fn get_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    let logs = super::logger::get_recent_logs();
    Ok(Json(logs))
}

/// Fetch the raw Config struct
pub async fn get_config(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;
    let cfg = Config::load(&state.config_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(cfg))
}

/// Update the global config file and dynamically reload clients
pub async fn put_config(
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

pub async fn reload_all_clients(
    clients: &Arc<Mutex<HashMap<String, ManagedClient>>>,
    config: &Config,
    global_shutdown: &Arc<Mutex<bool>>,
) -> Result<(), anyhow::Error> {
    let mut cls = clients.lock().await;

    // Gracefully stop old clients: end calls, stop RTP receivers, unregister
    for mc in cls.values() {
        *mc.active.lock().await = false;
        // Stop the RTP receiver background loop
        let client_guard = mc.client.lock().await;
        if let Some(ref rtp_recv) = client_guard.rtp_receiver {
            rtp_recv.stop();
        }
        // Try to unregister before dropping
        drop(client_guard);
        let _ = mc.client.lock().await.unregister().await;
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

pub async fn audio_ws_handler(
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

pub async fn handle_audio_ws(socket: WebSocket, state: AppState, account_name: String) {
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
