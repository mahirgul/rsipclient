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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_omits_unset_fields_on_the_wire() {
        let json = serde_json::to_string(&Request::new("status")).unwrap();
        assert_eq!(json, r#"{"cmd":"status"}"#);

        let json = serde_json::to_string(&Request::with_target("call", "main", "1002")).unwrap();
        assert_eq!(json, r#"{"cmd":"call","account":"main","target":"1002"}"#);
    }

    #[test]
    fn request_accepts_missing_optional_fields() {
        let req: Request = serde_json::from_str(r#"{"cmd":"register","account":"a"}"#).unwrap();
        assert_eq!(req.cmd, "register");
        assert_eq!(req.account.as_deref(), Some("a"));
        assert!(req.target.is_none());
        assert!(serde_json::from_str::<Request>(r#"{"account":"a"}"#).is_err());
    }

    #[test]
    fn response_constructors_set_the_ok_flag() {
        let ok = Response::ok("done");
        assert!(ok.ok);
        assert_eq!(ok.msg, "done");
        let fail = Response::fail("nope");
        assert!(!fail.ok);
        let back: Response = serde_json::from_str(&serde_json::to_string(&fail).unwrap()).unwrap();
        assert!(!back.ok);
        assert_eq!(back.msg, "nope");
    }
}
