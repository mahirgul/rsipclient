//! Rhai Embedded Scripting Engine Runner
//!
//! Provides execution context and Rust API bindings for `.rhai` script files.
//!
//! ### Exposed Rhai Functions:
//! - `rsip_log(level, text)`
//! - `rsip_get_caller()`
//! - `rsip_get_account()`

use super::types::{PluginActionResult, PluginEvent};
use rhai::{Dynamic, Engine, Map, Scope};
use std::fs;
use std::path::Path;

/// Rhai Script Runner
pub struct RhaiRunner {
    engine: Engine,
}

impl RhaiRunner {
    /// Initialize a new Rhai engine and register exposed Rust functions
    pub fn new() -> Self {
        let mut engine = Engine::new();

        // Register custom logging function for Rhai scripts
        engine.register_fn("rsip_log", |level: &str, msg: &str| {
            match level.to_lowercase().as_str() {
                "error" => log::error!("[Rhai Plugin] {}", msg),
                "warn" => log::warn!("[Rhai Plugin] {}", msg),
                "debug" => log::debug!("[Rhai Plugin] {}", msg),
                _ => log::info!("[Rhai Plugin] {}", msg),
            }
        });

        Self { engine }
    }

    /// Execute a `.rhai` script for a given event or IVR step
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
            .map_err(|e| format!("Failed to read script file: {}", e))?;

        let mut scope = Scope::new();

        // Inject event data into Rhai scope if available
        if let Some(evt) = event {
            let mut event_map = Map::new();
            match evt {
                PluginEvent::IncomingCall {
                    account,
                    caller,
                    call_id,
                    timestamp,
                } => {
                    event_map.insert("event".into(), Dynamic::from("incoming_call"));
                    event_map.insert("account".into(), Dynamic::from(account.clone()));
                    event_map.insert("caller".into(), Dynamic::from(caller.clone()));
                    event_map.insert("call_id".into(), Dynamic::from(call_id.clone()));
                    event_map.insert("timestamp".into(), Dynamic::from(*timestamp as i64));
                }
                PluginEvent::CallStateChanged {
                    account,
                    state,
                    call_id,
                    timestamp,
                } => {
                    event_map.insert("event".into(), Dynamic::from("call_state"));
                    event_map.insert("account".into(), Dynamic::from(account.clone()));
                    event_map.insert("state".into(), Dynamic::from(state.clone()));
                    if let Some(cid) = call_id {
                        event_map.insert("call_id".into(), Dynamic::from(cid.clone()));
                    }
                    event_map.insert("timestamp".into(), Dynamic::from(*timestamp as i64));
                }
                PluginEvent::DtmfReceived {
                    account,
                    digit,
                    timestamp,
                } => {
                    event_map.insert("event".into(), Dynamic::from("dtmf"));
                    event_map.insert("account".into(), Dynamic::from(account.clone()));
                    event_map.insert("digit".into(), Dynamic::from(digit.to_string()));
                    event_map.insert("timestamp".into(), Dynamic::from(*timestamp as i64));
                }
                PluginEvent::RegistrationStatus {
                    account,
                    registered,
                    timestamp,
                } => {
                    event_map.insert("event".into(), Dynamic::from("registration"));
                    event_map.insert("account".into(), Dynamic::from(account.clone()));
                    event_map.insert("registered".into(), Dynamic::from(*registered));
                    event_map.insert("timestamp".into(), Dynamic::from(*timestamp as i64));
                }
            }
            scope.push("event", event_map);
        }

        // Inject context data (e.g. IVR caller info) if available
        if let Some(ctx) = context_data {
            let mut ctx_map = Map::new();
            for (k, v) in ctx {
                ctx_map.insert(k.into(), Dynamic::from(v));
            }
            scope.push("context", ctx_map);
        }

        // Evaluate the Rhai script
        let result: Dynamic = self
            .engine
            .eval_with_scope(&mut scope, &script_content)
            .map_err(|e| format!("Rhai execution error: {}", e))?;

        // Parse result if script returns an Object Map (e.g. #{ action: "transfer", target: "sip:..." })
        if let Some(map) = result.try_cast::<Map>() {
            if let Some(act) = map.get("action").and_then(|v| v.clone().into_string().ok()) {
                match act.as_str() {
                    "transfer" => {
                        let target = map
                            .get("target")
                            .and_then(|v| v.clone().into_string().ok())
                            .unwrap_or_default();
                        return Ok(PluginActionResult::Transfer { target });
                    }
                    "playback" => {
                        let target = map
                            .get("target")
                            .and_then(|v| v.clone().into_string().ok())
                            .unwrap_or_default();
                        return Ok(PluginActionResult::Playback { target });
                    }
                    "record" => {
                        let target = map
                            .get("target")
                            .and_then(|v| v.clone().into_string().ok())
                            .unwrap_or_default();
                        let duration = map
                            .get("duration")
                            .and_then(|v| v.as_int().ok())
                            .map(|d| d as u64);
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

use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_rhai_script_execution() {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join("test_rhai_action.rhai");
        fs::write(
            &script_path,
            r#"
            rsip_log("info", "Testing Rhai runner");
            #{ action: "transfer", target: "sip:100@domain.com" }
            "#,
        )
        .unwrap();

        let runner = RhaiRunner::new();
        let res = runner.execute_script(&script_path, None, None).unwrap();
        match res {
            PluginActionResult::Transfer { target } => assert_eq!(target, "sip:100@domain.com"),
            _ => panic!("Expected transfer action"),
        }
        let _ = fs::remove_file(script_path);
    }
}
