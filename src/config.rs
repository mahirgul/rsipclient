//! Configuration module.
//!
//! Defines the structure and default values of the client configuration,
//! including web server settings, command API config, accounts, and audio settings.

use serde::{Deserialize, Serialize};
use std::fs;

/// Configuration for the web dashboard service
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct WebConfig {
    /// Port to listen on (default: 9090)
    #[serde(default = "default_web_port")]
    pub port: u16,

    /// Username for dashboard login (default: "admin")
    #[serde(default = "default_web_username")]
    pub username: String,

    /// Password for dashboard login (default: "admin")
    #[serde(default = "default_web_password")]
    pub password: String,
}

/// Configuration for the REST commands service
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct CommandsApiConfig {
    /// Port to listen on (default: 9099)
    #[serde(default = "default_commands_port")]
    pub port: u16,

    /// Optional username for REST commands API (uses web username if not specified)
    pub username: Option<String>,

    /// Optional password for REST commands API (uses web password if not specified)
    pub password: Option<String>,
}

fn default_commands_port() -> u16 {
    9099
}

fn default_web_port() -> u16 {
    9090
}

fn default_web_username() -> String {
    "admin".to_string()
}

fn default_web_password() -> String {
    "admin".to_string()
}

/// Root config structure loaded from TOML file
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Config {
    /// Optional web server settings
    pub web: Option<WebConfig>,

    /// Optional REST commands API settings
    pub commands_api: Option<CommandsApiConfig>,

    /// List of SIP accounts
    pub accounts: Vec<Account>,
}

/// A single SIP account configuration
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Account {
    // ── Required fields ─────────────────────────────────
    /// Display name for this account (used in CLI selection)
    pub name: String,

    /// SIP username / extension
    pub username: String,

    /// SIP password
    pub password: String,

    /// SIP server address (host:port), e.g. "192.168.1.1:5060"
    pub server: String,

    // ── Basic optional ──────────────────────────────────
    /// SIP domain
    #[serde(default = "default_domain")]
    pub domain: String,

    /// Local SIP port (UDP) for this account (0 = OS picks)
    #[serde(default = "default_sip_port")]
    pub sip_port: u16,

    /// RTP port range start (audio stream)
    #[serde(default = "default_rtp_port_start")]
    pub rtp_port_start: u16,

    /// RTP port range end (inclusive)
    #[serde(default = "default_rtp_port_end")]
    pub rtp_port_end: u16,

    /// Authentication method: "md5" (default) or "none"
    #[serde(default = "default_auth_method")]
    pub auth_method: Option<String>,

    /// Preferred codec: "pcmu", "pcma", "opus" (default: "pcmu")
    #[serde(default = "default_codec")]
    pub codec: Option<String>,

    /// SIP transport: "udp" (default) or "tls"
    #[serde(default = "default_transport")]
    pub transport: Option<String>,

    // ── Identity ────────────────────────────────────────
    /// Display name in From header, e.g. "Alice Smith"
    pub display_name: Option<String>,

    /// P-Asserted-Identity URI (RFC 3325), e.g. "sip:+44123456789@operator.com"
    pub asserted_id: Option<String>,

    /// P-Preferred-Identity URI (RFC 3325), e.g. "sip:alice@example.com"
    pub preferred_id: Option<String>,

    // ── Routing ─────────────────────────────────────────
    /// Outbound proxy (host:port), bypasses direct server routing
    pub proxy: Option<String>,

    // ── Timing ──────────────────────────────────────────
    /// Registration expiry in seconds (default: 3600 = 1 hour)
    #[serde(default = "default_register_expiry")]
    pub register_expiry: Option<u32>,

    /// Registration retry interval in seconds when registration fails (default: 30)
    #[serde(default = "default_register_retry_interval")]
    pub register_retry_interval: Option<u32>,

    // ── Protocol ────────────────────────────────────────
    /// Custom User-Agent header value
    pub user_agent: Option<String>,

    /// DTMF mode: "rfc2833", "inband", or "info"
    pub dtmf_mode: Option<String>,

    /// Accept early media (183 Session Progress with SDP)
    #[serde(default = "default_early_media")]
    pub early_media: Option<bool>,

    /// Enable RFC 4028 session timers (Session-Expires header)
    #[serde(default = "default_session_timers")]
    pub session_timers: Option<bool>,

    // ── IVR / Auto-Attendant ─────────────────────────────
    /// Auto-answer incoming INVITEs
    #[serde(default = "default_auto_answer")]
    pub auto_answer: Option<bool>,

    /// Path to IVR welcome WAV file (8kHz 16-bit mono PCM)
    pub ivr_welcome: Option<String>,

    /// IVR DTMF timeout in seconds (default: 10)
    #[serde(default = "default_ivr_timeout")]
    pub ivr_timeout: Option<u64>,

    /// IVR menu: DTMF digit → action string
    /// Format per action:
    ///   "transfer:sip:target@host"   — blind transfer
    ///   "playback:path/to/file.wav"  — play audio then return
    ///   "record:output.wav:30"       — record 30 seconds
    ///   "hold"                       — hold, press any key to resume
    ///   "hangup"                     — end call
    #[serde(default)]
    pub ivr_menu: Option<std::collections::HashMap<String, String>>,

    /// Default IVR action if no DTMF input
    pub ivr_default: Option<String>,
}

// ── Defaults ─────────────────────────────────────────────

fn default_domain() -> String {
    "localhost".to_string()
}

fn default_sip_port() -> u16 {
    0
}

fn default_rtp_port_start() -> u16 {
    8000
}

fn default_rtp_port_end() -> u16 {
    8010
}

fn default_auth_method() -> Option<String> {
    Some("md5".to_string())
}

fn default_codec() -> Option<String> {
    Some("pcmu".to_string())
}

fn default_transport() -> Option<String> {
    Some("udp".to_string())
}

fn default_register_expiry() -> Option<u32> {
    Some(3600)
}

fn default_register_retry_interval() -> Option<u32> {
    Some(30)
}

fn default_early_media() -> Option<bool> {
    Some(true)
}

fn default_session_timers() -> Option<bool> {
    Some(false)
}

fn default_auto_answer() -> Option<bool> {
    Some(false)
}

fn default_ivr_timeout() -> Option<u64> {
    Some(10)
}

// ── Validation ───────────────────────────────────────────

impl Config {
    /// Load configuration from a TOML file
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// Save configuration to a TOML file
    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    /// Validate configuration
    fn validate(&self) -> anyhow::Result<()> {
        if self.accounts.is_empty() {
            anyhow::bail!("Config must contain at least one account");
        }

        for (i, account) in self.accounts.iter().enumerate() {
            if account.username.is_empty() {
                anyhow::bail!("Account #{} ({}) has empty username", i, account.name);
            }
            if account.server.is_empty() {
                anyhow::bail!("Account #{} ({}) has empty server", i, account.name);
            }
            if account.rtp_port_start > account.rtp_port_end {
                anyhow::bail!(
                    "Account #{} ({}) rtp_port_start ({}) > rtp_port_end ({})",
                    i,
                    account.name,
                    account.rtp_port_start,
                    account.rtp_port_end
                );
            }
            if let Some(ref dtmf) = account.dtmf_mode {
                let dm = dtmf.to_lowercase();
                if dm != "rfc2833" && dm != "inband" && dm != "info" {
                    anyhow::bail!(
                        "Account #{} ({}) dtmf_mode must be rfc2833/inband/info, got '{}'",
                        i,
                        account.name,
                        dtmf
                    );
                }
            }

            // Validate transport
            if let Some(ref transport) = account.transport {
                let t = transport.to_lowercase();
                if t != "udp" && t != "tls" {
                    anyhow::bail!(
                        "Account #{} ({}) transport must be udp or tls, got '{}'",
                        i,
                        account.name,
                        transport
                    );
                }
            }

            // Warn if port ranges overlap
            for (j, other) in self.accounts.iter().enumerate() {
                if i != j
                    && ranges_overlap(
                        account.rtp_port_start,
                        account.rtp_port_end,
                        other.rtp_port_start,
                        other.rtp_port_end,
                    )
                {
                    log::warn!(
                        "RTP port ranges overlap: '{}' ({}-{}) and '{}' ({}-{})",
                        account.name,
                        account.rtp_port_start,
                        account.rtp_port_end,
                        other.name,
                        other.rtp_port_start,
                        other.rtp_port_end
                    );
                }
                if i != j && account.sip_port != 0 && account.sip_port == other.sip_port {
                    log::warn!(
                        "SIP ports collide: '{}' and '{}' both use port {}",
                        account.name,
                        other.name,
                        account.sip_port
                    );
                }
            }
        }
        Ok(())
    }
}

fn ranges_overlap(a_start: u16, a_end: u16, b_start: u16, b_end: u16) -> bool {
    a_start <= b_end && b_start <= a_end
}
