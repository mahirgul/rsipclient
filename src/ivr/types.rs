//! IVR data structures and action types.
//!
//! Defines the actions (Playback, Record, Transfer, Hold, Hangup)
//! and the menus mapped to DTMF digits.

use std::collections::HashMap;

/// IVR action after DTMF input
#[derive(Clone, Debug)]
pub enum IvrAction {
    /// Transfer to a SIP URI (sends REFER)
    Transfer(String),
    /// Play a WAV file, then return to menu
    Playback(String),
    /// Record caller audio, save to WAV
    Record { path: String, duration_secs: u64 },
    /// Put call on hold, press any DTMF to resume
    Hold,
    /// Hang up
    Hangup,
}

/// DTMF to action mapping for a menu
pub type IvrMenu = HashMap<char, IvrAction>;

/// IVR configuration
#[derive(Clone)]
pub struct IvrConfig {
    /// Path to welcome WAV file (8kHz, 16-bit, mono PCM)
    pub welcome_file: String,
    /// Max time to wait for DTMF input (seconds)
    pub timeout_secs: u64,
    /// Max DTMF digits to collect per menu
    pub max_digits: usize,
    /// DTMF to action map
    pub menu: IvrMenu,
    /// Default action if no DTMF or invalid input
    pub default_action: Option<IvrAction>,
}
