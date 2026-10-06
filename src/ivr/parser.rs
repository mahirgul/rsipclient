//! IVR configuration and action parser.
//!
//! Parses action strings (like "playback:", "record:", or "transfer:") and DTMF digit mappings
//! from configuration to construct the active IVR menu logic.

use crate::config::Account;
use crate::ivr::types::{IvrAction, IvrConfig, IvrMenu};
use std::collections::HashMap;

/// Recording length used when a `record:` action does not specify one.
const DEFAULT_RECORD_SECS: u64 = 10;

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
        // An optional trailing ":<seconds>" sets the duration, e.g.
        // "voicemail.wav:30". Only split when the suffix is a number, so that
        // colons inside the path ("C:\\voicemail.wav") are left alone.
        match rest
            .rsplit_once(':')
            .and_then(|(path, secs)| Some((path, secs.trim().parse::<u64>().ok()?)))
        {
            Some((path, secs)) => IvrAction::Record {
                path: path.to_string(),
                duration_secs: secs,
            },
            None => IvrAction::Record {
                path: rest.to_string(),
                duration_secs: DEFAULT_RECORD_SECS,
            },
        }
    } else if let Some(url) = s.strip_prefix("webhook:") {
        IvrAction::Webhook(url.to_string())
    } else if let Some(script_path) = s.strip_prefix("script:") {
        IvrAction::Script(script_path.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn record(action: IvrAction) -> (String, u64) {
        match action {
            IvrAction::Record {
                path,
                duration_secs,
            } => (path, duration_secs),
            other => panic!("expected Record, got {other:?}"),
        }
    }

    #[test]
    fn parses_simple_actions() {
        assert!(
            matches!(parse_action("transfer:sip:100@pbx"), IvrAction::Transfer(t) if t == "sip:100@pbx")
        );
        assert!(
            matches!(parse_action("playback:menu.wav"), IvrAction::Playback(p) if p == "menu.wav")
        );
        assert!(
            matches!(parse_action("webhook:https://x/y?a=b:c"), IvrAction::Webhook(u) if u == "https://x/y?a=b:c")
        );
        assert!(
            matches!(parse_action("script:plugins/a.lua"), IvrAction::Script(p) if p == "plugins/a.lua")
        );
        assert!(matches!(parse_action("hold"), IvrAction::Hold));
        assert!(matches!(parse_action("hangup"), IvrAction::Hangup));
    }

    #[test]
    fn unknown_actions_fall_back_to_hangup() {
        assert!(matches!(parse_action(""), IvrAction::Hangup));
        assert!(matches!(parse_action("Transfer:100"), IvrAction::Hangup));
        assert!(matches!(parse_action("dance"), IvrAction::Hangup));
    }

    #[test]
    fn record_reads_an_optional_duration() {
        assert_eq!(
            record(parse_action("record:vm.wav:30")),
            ("vm.wav".into(), 30)
        );
        assert_eq!(record(parse_action("record:vm.wav")), ("vm.wav".into(), 10));
        assert_eq!(
            record(parse_action("record:dir/sub/vm.wav:60")),
            ("dir/sub/vm.wav".into(), 60)
        );
    }

    #[test]
    fn record_keeps_colons_that_are_part_of_the_path() {
        assert_eq!(
            record(parse_action(r"record:C:\voicemail\vm.wav")),
            (r"C:\voicemail\vm.wav".into(), 10)
        );
        assert_eq!(
            record(parse_action(r"record:C:\voicemail\vm.wav:45")),
            (r"C:\voicemail\vm.wav".into(), 45)
        );
        assert_eq!(
            record(parse_action("record:vm.wav:abc")),
            ("vm.wav:abc".into(), 10)
        );
    }

    #[test]
    fn parse_menu_maps_first_character_of_each_key() {
        let raw = HashMap::from([
            ("1".to_string(), "transfer:200".to_string()),
            ("#".to_string(), "hangup".to_string()),
            ("9".to_string(), "hold".to_string()),
        ]);
        let menu = parse_menu(&raw);
        assert_eq!(menu.len(), 3);
        assert!(matches!(&menu[&'1'], IvrAction::Transfer(t) if t == "200"));
        assert!(matches!(menu[&'#'], IvrAction::Hangup));
        assert!(matches!(menu[&'9'], IvrAction::Hold));
    }

    #[test]
    fn build_ivr_config_requires_a_welcome_file() {
        let mut account: Account = toml::from_str(
            r#"
            name = "a"
            username = "u"
            password = "p"
            server = "127.0.0.1:5060"
            "#,
        )
        .unwrap();
        account.ivr_welcome = None;
        assert!(build_ivr_config(&account).is_none());

        account.ivr_welcome = Some("welcome.wav".into());
        account.ivr_timeout = Some(5);
        account.ivr_menu = Some(HashMap::from([("1".into(), "playback:x.wav".into())]));
        account.ivr_default = Some("transfer:operator".into());
        let cfg = build_ivr_config(&account).unwrap();
        assert_eq!(cfg.welcome_file, "welcome.wav");
        assert_eq!(cfg.timeout_secs, 5);
        assert_eq!(cfg.max_digits, 4);
        assert_eq!(cfg.menu.len(), 1);
        assert!(matches!(cfg.default_action, Some(IvrAction::Transfer(ref t)) if t == "operator"));
    }
}
