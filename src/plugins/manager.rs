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
        let path = resolve_script_path(&cfg.script_dir, filename)?;
        fs::read_to_string(path).map_err(|e| format!("Could not read file: {}", e))
    }

    /// Save or update content of a script file (.rhai or .lua)
    pub async fn save_script_file(&self, filename: &str, content: &str) -> Result<(), String> {
        let cfg = self.config.lock().await;
        let path = resolve_script_path(&cfg.script_dir, filename)?;
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

/// Reject anything but a plain filename, preventing path traversal / absolute-path
/// escapes out of the configured script directory (e.g. "../../etc/passwd", "/etc/passwd", "C:\\...").
fn is_safe_script_filename(filename: &str) -> bool {
    !filename.is_empty()
        && filename != "."
        && filename != ".."
        && !filename.contains('/')
        && !filename.contains('\\')
}

fn has_script_extension(filename: &str) -> bool {
    let lower = filename.to_lowercase();
    lower.ends_with(".rhai") || lower.ends_with(".lua")
}

/// Resolve `filename` to a path inside the configured script directory.
///
/// Restricting the filename is not enough on its own: `script_dir` is itself
/// settable through the dashboard, and a symlink dropped into it points
/// wherever it likes. Require the extension on reads as well as writes, and
/// require the resolved path to stay under the resolved script directory.
fn resolve_script_path(script_dir: &str, filename: &str) -> Result<PathBuf, String> {
    if !is_safe_script_filename(filename) {
        return Err("Invalid filename".to_string());
    }
    if !has_script_extension(filename) {
        return Err("Filename must end with .rhai or .lua".to_string());
    }

    let base = Path::new(script_dir)
        .canonicalize()
        .map_err(|e| format!("Script directory is not accessible: {}", e))?;
    let path = base.join(filename);

    match path.canonicalize() {
        // The file exists — it must resolve to somewhere under the script dir,
        // which a symlink pointing outside would not.
        Ok(resolved) if resolved.starts_with(&base) => Ok(resolved),
        Ok(_) => Err("Invalid filename".to_string()),
        // Not created yet: `base` is canonical and the filename carries no
        // separators, so the join cannot leave the directory.
        Err(_) => Ok(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create an empty directory under the system temp dir, unique per test.
    fn temp_script_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rsipclient-scripts-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn accepts_a_plain_script_name() {
        let dir = temp_script_dir("plain");
        let path = resolve_script_path(dir.to_str().unwrap(), "ivr.lua").expect("should resolve");
        assert_eq!(path, dir.canonicalize().unwrap().join("ivr.lua"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_path_separators_and_traversal() {
        let dir = temp_script_dir("traversal");
        let d = dir.to_str().unwrap();
        for bad in [
            "../secrets.lua",
            "sub/ivr.lua",
            "sub\\ivr.lua",
            "..",
            ".",
            "",
        ] {
            assert!(
                resolve_script_path(d, bad).is_err(),
                "{} should be rejected",
                bad
            );
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// `script_dir` is settable through the dashboard, so without an extension
    /// check on reads it doubled as an arbitrary file reader.
    #[test]
    fn rejects_names_that_are_not_scripts() {
        let dir = temp_script_dir("ext");
        let d = dir.to_str().unwrap();
        assert!(resolve_script_path(d, "shadow").is_err());
        assert!(resolve_script_path(d, "id_rsa").is_err());
        assert!(resolve_script_path(d, "notes.txt").is_err());
        assert!(
            resolve_script_path(d, "ivr.LUA").is_ok(),
            "extension check is case-insensitive"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// A symlink dropped in the script directory still has a bare, correctly
    /// suffixed name — only resolving it catches where it actually points.
    #[cfg(unix)]
    #[test]
    fn rejects_a_symlink_out_of_the_script_dir() {
        let dir = temp_script_dir("symlink");
        let outside =
            std::env::temp_dir().join(format!("rsipclient-outside-{}.lua", std::process::id()));
        fs::write(&outside, "-- secret").expect("write outside file");
        let link = dir.join("escape.lua");
        std::os::unix::fs::symlink(&outside, &link).expect("symlink");

        assert!(resolve_script_path(dir.to_str().unwrap(), "escape.lua").is_err());

        let _ = fs::remove_file(&outside);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_a_missing_script_dir() {
        let missing = std::env::temp_dir().join("rsipclient-no-such-dir-xyz");
        let _ = fs::remove_dir_all(&missing);
        assert!(resolve_script_path(missing.to_str().unwrap(), "ivr.lua").is_err());
    }
}
