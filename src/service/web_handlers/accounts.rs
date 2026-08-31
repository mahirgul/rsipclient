//! Web Dashboard API handlers for SIP accounts.
//!
//! Provides handlers to add, edit, delete, register, and unregister SIP accounts dynamically.

use super::super::create_managed_client;
use super::super::web_server::{verify_token, AppState};
use crate::config::{Account, Config};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use std::sync::Arc;

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

    // Load config first: an account that failed to start at boot is absent from
    // `cls` but still present in the file, and appending it again would write a
    // duplicate.
    let mut cfg =
        Config::load(&state.config_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if cls.contains_key(&new_acc.name) || cfg.accounts.iter().any(|a| a.name == new_acc.name) {
        return Err(StatusCode::BAD_REQUEST);
    }

    cfg.accounts.push(new_acc.clone());
    cfg.validate().map_err(|e| {
        log::warn!("Rejected invalid account '{}': {}", new_acc.name, e);
        StatusCode::BAD_REQUEST
    })?;

    // Create the managed client before persisting, so a config that cannot be
    // run is never written to disk.
    let mc = create_managed_client(&new_acc).await.map_err(|e| {
        log::error!("Failed to create dynamic client: {}", e);
        StatusCode::BAD_REQUEST
    })?;

    cfg.save(&state.config_path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Spawn background watchers
    super::super::spawn_watchers_for_client(
        new_acc.name.clone(),
        &mc,
        state.global_shutdown.clone(),
    );

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

    // Load config and apply the edit in memory; nothing is written until the
    // replacement client is actually running.
    let mut cfg =
        Config::load(&state.config_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let old_account = match cfg.accounts.iter().position(|a| a.name == name) {
        Some(idx) => {
            let previous = cfg.accounts[idx].clone();
            cfg.accounts[idx] = updated_acc.clone();
            previous
        }
        None => return Err(StatusCode::NOT_FOUND),
    };
    cfg.validate().map_err(|e| {
        log::warn!("Rejected invalid account '{}': {}", name, e);
        StatusCode::BAD_REQUEST
    })?;

    // If the old client was successfully running/initialized, stop and drop it.
    // The replacement usually reuses the same ports, so the old one has to go
    // first — which is why a failure below has to put it back.
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
    let mc = match create_managed_client(&updated_acc).await {
        Ok(mc) => mc,
        Err(e) => {
            log::error!("Failed to create dynamic client: {}", e);
            // Put the previous account back so the edit fails cleanly instead
            // of leaving the account running nowhere.
            match create_managed_client(&old_account).await {
                Ok(restored) => {
                    super::super::spawn_watchers_for_client(
                        old_account.name.clone(),
                        &restored,
                        state.global_shutdown.clone(),
                    );
                    cls.insert(old_account.name.clone(), restored);
                }
                Err(e) => log::error!(
                    "Failed to restore previous client for '{}': {}",
                    old_account.name,
                    e
                ),
            }
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    // Spawn background watchers
    super::super::spawn_watchers_for_client(
        updated_acc.name.clone(),
        &mc,
        state.global_shutdown.clone(),
    );

    // Insert new (handles renaming)
    cls.insert(updated_acc.name.clone(), mc);

    cfg.save(&state.config_path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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
    cfg.validate().map_err(|e| {
        log::warn!("Refusing to delete account '{}': {}", name, e);
        StatusCode::BAD_REQUEST
    })?;
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
