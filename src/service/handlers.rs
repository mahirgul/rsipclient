//! Command handlers for the SIP service IPC
//!
//! Each function handles one command ("register", "call", "hangup", "cancel",
//! "status", "shutdown", "play") and returns a Response.

pub mod call;
pub mod registration;
pub mod status;

use crate::ipc::{Request, Response};
use std::collections::HashMap;

use super::ManagedClient;

/// Route a request to the correct handler
pub async fn process_command(req: &Request, clients: &HashMap<String, ManagedClient>) -> Response {
    match req.cmd.as_str() {
        "status" => status::handle_status(clients).await,
        "register" => registration::handle_register(req, clients).await,
        "unregister" => registration::handle_unregister(req, clients).await,
        "call" => call::handle_call(req, clients).await,
        "hangup" => call::handle_hangup(req, clients).await,
        "cancel" => call::handle_cancel(req, clients).await,
        "hold" => call::handle_hold(req, clients).await,
        "resume" => call::handle_resume(req, clients).await,
        "transfer" => call::handle_transfer(req, clients).await,
        "dtmf" => call::handle_dtmf(req, clients).await,
        "play" => call::handle_play(req, clients).await,
        "shutdown" => Response::ok("Shutting down..."),
        _ => Response::fail(&format!("Unknown command: '{}'", req.cmd)),
    }
}

// ── Helper ─────────────────────────────────────────────────

/// Validate account field exists and return the ManagedClient
pub(crate) fn get_account<'a>(
    req: &Request,
    cmd: &str,
    clients: &'a HashMap<String, ManagedClient>,
) -> Result<&'a ManagedClient, Response> {
    let account_name = match &req.account {
        Some(a) => a,
        None => {
            return Err(Response::fail(&format!(
                "'{}' requires 'account' field",
                cmd
            )))
        }
    };
    match clients.get(account_name) {
        Some(mc) => Ok(mc),
        None => Err(Response::fail(&format!(
            "Account '{}' not found",
            account_name
        ))),
    }
}
