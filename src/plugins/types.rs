//! Plugin Data Models and Types
//!
//! Provides the core data structures for events, webhook configurations,
//! script execution parameters, and plugin action results.
//!
//! Contributors can easily extend `PluginEvent` to add new event types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Client event types dispatched to plugins
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginEvent {
    /// Dispatched when an incoming call is received
    IncomingCall {
        account: String,
        caller: String,
        call_id: String,
        timestamp: u64,
    },
    /// Dispatched when call state changes (Answered, Held, Ended)
    CallStateChanged {
        account: String,
        call_id: Option<String>,
        state: String,
        timestamp: u64,
    },
    /// Dispatched when a DTMF digit keypress is received
    DtmfReceived {
        account: String,
        digit: char,
        timestamp: u64,
    },
    /// Dispatched when SIP registration status changes
    RegistrationStatus {
        account: String,
        registered: bool,
        timestamp: u64,
    },
}

#[allow(dead_code)]
impl PluginEvent {
    /// Get the event type string identifier for filtering
    pub fn event_type(&self) -> &'static str {
        match self {
            PluginEvent::IncomingCall { .. } => "incoming_call",
            PluginEvent::CallStateChanged { .. } => "call_state",
            PluginEvent::DtmfReceived { .. } => "dtmf",
            PluginEvent::RegistrationStatus { .. } => "registration",
        }
    }
}

/// Action returned by a script or webhook plugin
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PluginActionResult {
    /// Perform blind transfer to target SIP URI
    Transfer { target: String },
    /// Play WAV audio file
    Playback { target: String },
    /// Record audio to file
    Record {
        target: String,
        duration: Option<u64>,
    },
    /// Hold call
    Hold,
    /// Hang up call
    Hangup,
    /// No action / Continue normal flow
    None,
}

/// Webhook plugin configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebhookPluginConfig {
    /// Unique name of the webhook plugin
    pub name: String,
    /// Target HTTP URL to POST event JSON payloads
    pub url: String,
    /// List of subscribed event types ("incoming_call", "call_state", "dtmf", "registration")
    pub events: Vec<String>,
    /// Optional HTTP headers to include in POST requests
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    /// Optional timeout in milliseconds (default: 5000ms)
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Enabled status flag
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Root plugin system configuration saved in config.toml
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginSystemConfig {
    /// Enable or disable global plugin execution
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Directory path containing .rhai and .lua script files (default: "plugins")
    #[serde(default = "default_plugins_dir")]
    pub script_dir: String,
    /// List of configured Webhook plugins
    #[serde(default)]
    pub webhooks: Vec<WebhookPluginConfig>,
}

impl Default for PluginSystemConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            script_dir: default_plugins_dir(),
            webhooks: Vec::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_plugins_dir() -> String {
    "plugins".to_string()
}
