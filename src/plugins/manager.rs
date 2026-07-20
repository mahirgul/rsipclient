//! Plugin State Manager
//!
//! Manages active plugin configurations, script directory storage,
//! and orchestrates dispatching events to Rhai, Lua, and Webhook engines.

use super::lua_runner::LuaRunner;
use super::rhai_runner::RhaiRunner;
use super::types::{PluginActionResult, PluginEvent, PluginSystemConfig};
use super::webhook_runner::WebhookRunner;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared Plugin Manager State
#[allow(dead_code)]
#[derive(Clone)]
pub struct PluginManager {
    config: Arc<Mutex<PluginSystemConfig>>,
    rhai_runner: Arc<RhaiRunner>,
    lua_runner: Arc<LuaRunner>,
    webhook_runner: Arc<WebhookRunner>,
}

#[allow(dead_code)]
impl PluginManager {
    /// Create a new PluginManager with default or loaded config
    pub fn new(config: PluginSystemConfig) -> Self {
        // Ensure script directory exists
        let script_dir = Path::new(&config.script_dir);
        if !script_dir.exists() {
            let _ = fs::create_dir_all(script_dir);
        }

        Self {
            config: Arc::new(Mutex::new(config)),
            rhai_runner: Arc::new(RhaiRunner::new()),
            lua_runner: Arc::new(LuaRunner::new()),
            webhook_runner: Arc::new(WebhookRunner::new()),
        }
    }

    /// Dispatch a client event to all active script and webhook plugins
    pub async fn dispatch_event(&self, event: PluginEvent) {
        let cfg = self.config.lock().await.clone();
        if !cfg.enabled {
            return;
        }

        // 1. Dispatch to Webhooks asynchronously
        let webhook_runner = self.webhook_runner.clone();
        let event_clone = event.clone();
        tokio::spawn(async move {
            for wh in cfg.webhooks {
                webhook_runner.dispatch_event(&wh, &event_clone).await;
            }
        });

        // 2. Dispatch to local script files (.rhai and .lua) in script_dir
        let script_dir = PathBuf::from(&cfg.script_dir);
        if let Ok(entries) = fs::read_dir(&script_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        match ext {
                            "rhai" => {
                                let runner = self.rhai_runner.clone();
                                let evt = event.clone();
                                std::thread::spawn(move || {
                                    let _ = runner.execute_script(&path, Some(&evt), None);
                                });
                            }
                            "lua" => {
                                let runner = self.lua_runner.clone();
                                let evt = event.clone();
                                std::thread::spawn(move || {
                                    let _ = runner.execute_script(&path, Some(&evt), None);
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    /// Execute a script (Rhai or Lua based on file extension) for IVR steps
    pub fn execute_ivr_script(
        &self,
        script_path_str: &str,
        context: HashMap<String, String>,
    ) -> Result<PluginActionResult, String> {
        let path = Path::new(script_path_str);
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match ext.as_str() {
            "rhai" => self.rhai_runner.execute_script(path, None, Some(context)),
            "lua" => self.lua_runner.execute_script(path, None, Some(context)),
            _ => Err(format!(
                "Unsupported script extension '{}'. Supported extensions: .rhai, .lua",
                ext
            )),
        }
    }

    /// Execute an IVR Webhook query asynchronously
    pub async fn execute_ivr_webhook(
        &self,
        url: &str,
        context: HashMap<String, String>,
    ) -> Result<PluginActionResult, String> {
        self.webhook_runner
            .dispatch_ivr_webhook(url, &context)
            .await
    }

    /// List all script files in the script directory
    pub async fn list_script_files(&self) -> Vec<String> {
        let cfg = self.config.lock().await;
        let dir = Path::new(&cfg.script_dir);
        let mut files = Vec::new();
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if entry.path().is_file() {
                    if let Some(name) = entry.file_name().to_str() {
                        if name.ends_with(".rhai") || name.ends_with(".lua") {
                            files.push(name.to_string());
                        }
                    }
                }
            }
        }
        files
    }

    /// Read content of a script file
    pub async fn get_script_content(&self, filename: &str) -> Result<String, String> {
        let cfg = self.config.lock().await;
        let path = Path::new(&cfg.script_dir).join(filename);
        fs::read_to_string(path).map_err(|e| format!("Could not read file: {}", e))
    }

    /// Save or update content of a script file (.rhai or .lua)
    pub async fn save_script_file(&self, filename: &str, content: &str) -> Result<(), String> {
        if !filename.ends_with(".rhai") && !filename.ends_with(".lua") {
            return Err("Filename must end with .rhai or .lua".to_string());
        }
        let cfg = self.config.lock().await;
        let path = Path::new(&cfg.script_dir).join(filename);
        fs::write(path, content).map_err(|e| format!("Could not save file: {}", e))
    }

    /// Update plugin configurations
    pub async fn update_config(&self, new_config: PluginSystemConfig) {
        let mut cfg = self.config.lock().await;
        *cfg = new_config;
    }

    /// Get current plugin configuration
    pub async fn get_config(&self) -> PluginSystemConfig {
        self.config.lock().await.clone()
    }
}
