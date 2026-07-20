//! Lua 5.4 Embedded Scripting Engine Runner
//!
//! Provides execution context and Lua API bindings for `.lua` script files.
//!
//! ### Exposed Lua API (`rsip` table):
//! - `rsip.log(level, text)`

use super::types::{PluginActionResult, PluginEvent};
use mlua::{Lua, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Lua Script Runner
pub struct LuaRunner;

impl LuaRunner {
    pub fn new() -> Self {
        Self
    }

    /// Execute a `.lua` script for a given event or IVR step
    pub fn execute_script(
        &self,
        script_path: &Path,
        event: Option<&PluginEvent>,
        context_data: Option<HashMap<String, String>>,
    ) -> Result<PluginActionResult, String> {
        if !script_path.exists() {
            return Err(format!("Script file not found: {:?}", script_path));
        }

        let script_content = fs::read_to_string(script_path)
            .map_err(|e| format!("Failed to read Lua script file: {}", e))?;

        let lua = Lua::new();

        // Create 'rsip' library table for Lua scripts
        let globals = lua.globals();
        let rsip_table = lua.create_table().map_err(|e| e.to_string())?;

        // Register rsip.log function
        let log_fn = lua
            .create_function(|_, (level, msg): (String, String)| {
                match level.to_lowercase().as_str() {
                    "error" => log::error!("[Lua Plugin] {}", msg),
                    "warn" => log::warn!("[Lua Plugin] {}", msg),
                    "debug" => log::debug!("[Lua Plugin] {}", msg),
                    _ => log::info!("[Lua Plugin] {}", msg),
                }
                Ok(())
            })
            .map_err(|e| e.to_string())?;

        rsip_table.set("log", log_fn).map_err(|e| e.to_string())?;
        globals.set("rsip", rsip_table).map_err(|e| e.to_string())?;

        // Inject event table if present
        if let Some(evt) = event {
            let event_table = lua.create_table().map_err(|e| e.to_string())?;
            match evt {
                PluginEvent::IncomingCall {
                    account,
                    caller,
                    call_id,
                    timestamp,
                } => {
                    event_table.set("type", "incoming_call").ok();
                    event_table.set("account", account.as_str()).ok();
                    event_table.set("caller", caller.as_str()).ok();
                    event_table.set("call_id", call_id.as_str()).ok();
                    event_table.set("timestamp", *timestamp).ok();
                }
                PluginEvent::CallStateChanged {
                    account,
                    state,
                    call_id,
                    timestamp,
                } => {
                    event_table.set("type", "call_state").ok();
                    event_table.set("account", account.as_str()).ok();
                    event_table.set("state", state.as_str()).ok();
                    if let Some(cid) = call_id {
                        event_table.set("call_id", cid.as_str()).ok();
                    }
                    event_table.set("timestamp", *timestamp).ok();
                }
                PluginEvent::DtmfReceived {
                    account,
                    digit,
                    timestamp,
                } => {
                    event_table.set("type", "dtmf").ok();
                    event_table.set("account", account.as_str()).ok();
                    event_table.set("digit", digit.to_string()).ok();
                    event_table.set("timestamp", *timestamp).ok();
                }
                PluginEvent::RegistrationStatus {
                    account,
                    registered,
                    timestamp,
                } => {
                    event_table.set("type", "registration").ok();
                    event_table.set("account", account.as_str()).ok();
                    event_table.set("registered", *registered).ok();
                    event_table.set("timestamp", *timestamp).ok();
                }
            }
            globals
                .set("event", event_table)
                .map_err(|e| e.to_string())?;
        }

        // Inject context table if present
        if let Some(ctx) = context_data {
            let ctx_table = lua.create_table().map_err(|e| e.to_string())?;
            for (k, v) in ctx {
                ctx_table.set(k, v).ok();
            }
            globals
                .set("context", ctx_table)
                .map_err(|e| e.to_string())?;
        }

        // Execute Lua script
        let result: Value = lua
            .load(&script_content)
            .eval()
            .map_err(|e| format!("Lua execution error: {}", e))?;

        // Parse returned table action if any
        if let Value::Table(tbl) = result {
            if let Ok(act) = tbl.get::<_, String>("action") {
                match act.as_str() {
                    "transfer" => {
                        let target = tbl.get::<_, String>("target").unwrap_or_default();
                        return Ok(PluginActionResult::Transfer { target });
                    }
                    "playback" => {
                        let target = tbl.get::<_, String>("target").unwrap_or_default();
                        return Ok(PluginActionResult::Playback { target });
                    }
                    "record" => {
                        let target = tbl.get::<_, String>("target").unwrap_or_default();
                        let duration = tbl.get::<_, u64>("duration").ok();
                        return Ok(PluginActionResult::Record { target, duration });
                    }
                    "hold" => return Ok(PluginActionResult::Hold),
                    "hangup" => return Ok(PluginActionResult::Hangup),
                    _ => {}
                }
            }
        }

        Ok(PluginActionResult::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_lua_script_execution() {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join("test_lua_action.lua");
        fs::write(
            &script_path,
            r#"
            rsip.log("info", "Testing Lua runner")
            return { action = "playback", target = "test.wav" }
            "#,
        )
        .unwrap();

        let runner = LuaRunner::new();
        let res = runner.execute_script(&script_path, None, None).unwrap();
        match res {
            PluginActionResult::Playback { target } => assert_eq!(target, "test.wav"),
            _ => panic!("Expected playback action"),
        }
        let _ = fs::remove_file(script_path);
    }
}
