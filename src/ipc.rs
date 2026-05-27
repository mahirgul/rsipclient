//! IPC protocol types for CLI ↔ Service communication

use serde::{Deserialize, Serialize};

/// Command sent from CLI to the running service
#[derive(Serialize, Deserialize, Debug)]
pub struct Request {
    pub cmd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// Response sent from service back to CLI
#[derive(Serialize, Deserialize, Debug)]
pub struct Response {
    pub ok: bool,
    pub msg: String,
}

impl Request {
    pub fn new(cmd: &str) -> Self {
        Request {
            cmd: cmd.to_string(),
            account: None,
            target: None,
        }
    }

    pub fn with_account(cmd: &str, account: &str) -> Self {
        Request {
            cmd: cmd.to_string(),
            account: Some(account.to_string()),
            target: None,
        }
    }

    pub fn with_target(cmd: &str, account: &str, target: &str) -> Self {
        Request {
            cmd: cmd.to_string(),
            account: Some(account.to_string()),
            target: Some(target.to_string()),
        }
    }
}

impl Response {
    pub fn ok(msg: &str) -> Self {
        Response {
            ok: true,
            msg: msg.to_string(),
        }
    }

    pub fn fail(msg: &str) -> Self {
        Response {
            ok: false,
            msg: msg.to_string(),
        }
    }
}

/// Default control port the service listens on
pub const DEFAULT_CONTROL_PORT: u16 = 5090;
