//! Web Dashboard API Handlers for Plugin System
//!
//! Provides endpoints to retrieve, add, edit, test, and delete script files
//! (.rhai and .lua) as well as HTTP Webhook plugin configurations.

use super::super::web_server::{verify_token, AppState};
use crate::plugins::PluginSystemConfig;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct PluginStatusResponse {
    pub config: PluginSystemConfig,
    pub script_files: Vec<String>,
}

#[derive(Deserialize)]
pub struct SaveScriptPayload {
    pub filename: String,
    pub content: String,
}

/// Fetch active plugin config and script file list
pub async fn get_plugins_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;
    let config = state.plugin_manager.get_config().await;
    let script_files = state.plugin_manager.list_script_files().await;
    Ok(Json(PluginStatusResponse {
        config,
        script_files,
    }))
}

/// Update global plugin settings and webhooks list
pub async fn update_plugins_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(new_cfg): Json<PluginSystemConfig>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;
    state.plugin_manager.update_config(new_cfg).await;
    Ok(Json(serde_json::json!({ "success": true })))
}

/// Read content of a script file
pub async fn get_script_content(
    State(state): State<AppState>,
    Path(filename): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;
    match state.plugin_manager.get_script_content(&filename).await {
        Ok(content) => Ok(Json(
            serde_json::json!({ "success": true, "content": content }),
        )),
        Err(_) => Err(StatusCode::NOT_FOUND),
    }
}

/// Save or update a script file (.rhai or .lua)
pub async fn save_script_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SaveScriptPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;
    match state
        .plugin_manager
        .save_script_file(&payload.filename, &payload.content)
        .await
    {
        Ok(_) => Ok(Json(serde_json::json!({ "success": true }))),
        Err(e) => Ok(Json(serde_json::json!({ "success": false, "msg": e }))),
    }
}
