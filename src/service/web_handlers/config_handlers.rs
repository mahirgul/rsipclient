use super::super::web_server::{verify_token, AppState};
use super::super::{create_managed_client, ManagedClient};
use crate::config::Config;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

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
                super::super::incoming_call_watcher(
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
            super::super::registration_watcher(
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
