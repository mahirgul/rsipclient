//! IVR configuration and action parser.
//!
//! Parses action strings (like "playback:", "record:", or "transfer:") and DTMF digit mappings
//! from configuration to construct the active IVR menu logic.

use crate::config::Account;
use crate::ivr::types::{IvrAction, IvrConfig, IvrMenu};
use std::collections::HashMap;

/// Build an IVR menu from config key-value pairs
pub fn parse_menu(raw: &HashMap<String, String>) -> IvrMenu {
    let mut menu = IvrMenu::new();
    for (key, value) in raw {
        let digit = key.chars().next().unwrap_or(' ');
        let action = parse_action(value);
        menu.insert(digit, action);
    }
    menu
}

/// Parse a single action string into IvrAction
pub fn parse_action(s: &str) -> IvrAction {
    if let Some(target) = s.strip_prefix("transfer:") {
        IvrAction::Transfer(target.to_string())
    } else if let Some(path) = s.strip_prefix("playback:") {
        IvrAction::Playback(path.to_string())
    } else if let Some(rest) = s.strip_prefix("record:") {
        // Use rsplit_once to correctly split path and duration
        // e.g. "voicemail.wav:30" or "path/to/file.wav:60"
        if let Some((path, secs_str)) = rest.rsplit_once(':') {
            let secs = secs_str.parse().ok().unwrap_or(10);
            IvrAction::Record {
                path: path.to_string(),
                duration_secs: secs,
            }
        } else {
            IvrAction::Record {
                path: rest.to_string(),
                duration_secs: 10,
            }
        }
    } else if s == "hold" {
        IvrAction::Hold
    } else {
        IvrAction::Hangup
    }
}

/// Build IVR config from account settings
pub fn build_ivr_config(account: &Account) -> Option<IvrConfig> {
    let welcome = account.ivr_welcome.clone()?;
    let raw_menu = account.ivr_menu.clone().unwrap_or_default();
    let timeout = account.ivr_timeout.unwrap_or(10);
    let menu = parse_menu(&raw_menu);
    let default = account.ivr_default.as_ref().map(|s| parse_action(s));

    Some(IvrConfig {
        welcome_file: welcome,
        timeout_secs: timeout,
        max_digits: 4,
        menu,
        default_action: default,
    })
}
