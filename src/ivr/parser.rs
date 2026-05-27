use std::collections::HashMap;
use crate::config::Account;
use crate::ivr::types::{IvrAction, IvrConfig, IvrMenu};

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
    let parts: Vec<&str> = s.splitn(3, ':').collect();
    match parts.first().copied() {
        Some("transfer") => {
            let target = if parts.len() > 2 {
                format!("{}:{}", parts[1], parts[2])
            } else {
                parts.get(1).unwrap_or(&"").to_string()
            };
            IvrAction::Transfer(target)
        }
        Some("playback") => {
            let path = parts.get(1).unwrap_or(&"").to_string();
            IvrAction::Playback(path)
        }
        Some("record") => {
            let path = parts.get(1).unwrap_or(&"recording.wav").to_string();
            let secs: u64 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);
            IvrAction::Record {
                path,
                duration_secs: secs,
            }
        }
        Some("hold") => IvrAction::Hold,
        Some("hangup") => IvrAction::Hangup,
        _ => IvrAction::Hangup,
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
