//! Command handlers for the SIP service IPC
//!
//! Each function handles one command ("register", "call", "hangup", "cancel",
//! "status", "shutdown", "play") and returns a Response.

use crate::ipc::{Request, Response};
use anyhow::Result;
use std::collections::HashMap;

use super::ManagedClient;

/// Route a request to the correct handler
pub async fn process_command(req: &Request, clients: &HashMap<String, ManagedClient>) -> Response {
    match req.cmd.as_str() {
        "status" => handle_status(clients).await,
        "register" => handle_register(req, clients).await,
        "unregister" => handle_unregister(req, clients).await,
        "call" => handle_call(req, clients).await,
        "hangup" => handle_hangup(req, clients).await,
        "cancel" => handle_cancel(req, clients).await,
        "play" => handle_play(req, clients).await,
        "shutdown" => Response::ok("Shutting down..."),
        _ => Response::fail(&format!("Unknown command: '{}'", req.cmd)),
    }
}

// ── Individual handlers ────────────────────────────────────

async fn handle_status(clients: &HashMap<String, ManagedClient>) -> Response {
    let mut lines = vec![];
    for (name, mc) in clients {
        let client = mc.client.lock().await;
        let state = if client.in_call { "in call" } else { "idle" };
        lines.push(format!(
            "  {}: {}@{} bound={} {}",
            name, client.username, client.domain, client.local_addr, state
        ));
    }
    Response::ok(&format!("Accounts:\n{}", lines.join("\n")))
}

async fn handle_register(req: &Request, clients: &HashMap<String, ManagedClient>) -> Response {
    let account_name = get_account(req, "register", clients);
    let mc = match account_name {
        Ok(name) => name,
        Err(resp) => return resp,
    };
    *mc.should_register.lock().await = true;
    let client = mc.client.lock().await;
    match client.register().await {
        Ok(true) => Response::ok(&format!("'{}' registered", req.account.as_deref().unwrap())),
        Ok(false) => Response::fail(&format!(
            "'{}' registration failed",
            req.account.as_deref().unwrap()
        )),
        Err(e) => Response::fail(&format!(
            "'{}' error: {}",
            req.account.as_deref().unwrap(),
            e
        )),
    }
}

async fn handle_unregister(req: &Request, clients: &HashMap<String, ManagedClient>) -> Response {
    let account_name = get_account(req, "unregister", clients);
    let mc = match account_name {
        Ok(name) => name,
        Err(resp) => return resp,
    };
    *mc.should_register.lock().await = false;
    let client = mc.client.lock().await;
    match client.unregister().await {
        Ok(true) => Response::ok(&format!(
            "'{}' unregistered",
            req.account.as_deref().unwrap()
        )),
        Ok(false) => Response::fail(&format!(
            "'{}' unregistration failed",
            req.account.as_deref().unwrap()
        )),
        Err(e) => Response::fail(&format!(
            "'{}' error: {}",
            req.account.as_deref().unwrap(),
            e
        )),
    }
}

async fn handle_call(req: &Request, clients: &HashMap<String, ManagedClient>) -> Response {
    let target = match &req.target {
        Some(t) => t.clone(),
        None => return Response::fail("'call' requires 'target' field"),
    };
    let account_name = get_account(req, "call", clients);
    let mc = match account_name {
        Ok(name) => name,
        Err(resp) => return resp,
    };
    let mut client = mc.client.lock().await;
    match client.invite(&target).await {
        Ok(true) => Response::ok(&format!(
            "'{}' calling {} - established",
            req.account.as_deref().unwrap(),
            target
        )),
        Ok(false) => Response::fail(&format!(
            "'{}' call to {} failed",
            req.account.as_deref().unwrap(),
            target
        )),
        Err(e) => Response::fail(&format!(
            "'{}' error: {}",
            req.account.as_deref().unwrap(),
            e
        )),
    }
}

async fn handle_hangup(req: &Request, clients: &HashMap<String, ManagedClient>) -> Response {
    let account_name = get_account(req, "hangup", clients);
    let mc = match account_name {
        Ok(name) => name,
        Err(resp) => return resp,
    };
    let mut client = mc.client.lock().await;
    match client.bye().await {
        Ok(true) => Response::ok(&format!("'{}' call ended", req.account.as_deref().unwrap())),
        Ok(false) => Response::fail(&format!(
            "'{}' no active call",
            req.account.as_deref().unwrap()
        )),
        Err(e) => Response::fail(&format!(
            "'{}' error: {}",
            req.account.as_deref().unwrap(),
            e
        )),
    }
}

async fn handle_cancel(req: &Request, clients: &HashMap<String, ManagedClient>) -> Response {
    let account_name = get_account(req, "cancel", clients);
    let mc = match account_name {
        Ok(name) => name,
        Err(resp) => return resp,
    };
    let client = mc.client.lock().await;
    match client.cancel().await {
        Ok(true) => Response::ok(&format!("'{}' cancelled", req.account.as_deref().unwrap())),
        Ok(false) => Response::fail(&format!(
            "'{}' cancel failed",
            req.account.as_deref().unwrap()
        )),
        Err(e) => Response::fail(&format!(
            "'{}' error: {}",
            req.account.as_deref().unwrap(),
            e
        )),
    }
}

async fn handle_play(req: &Request, clients: &HashMap<String, ManagedClient>) -> Response {
    let account_name = match &req.account {
        Some(a) => a.clone(),
        None => return Response::fail("'play' requires 'account' field"),
    };
    let wav_path = match &req.target {
        Some(t) => t.clone(),
        None => return Response::fail("'play' requires 'target' (WAV file path) field"),
    };
    let mc = match clients.get(&account_name) {
        Some(m) => m,
        None => return Response::fail(&format!("Account '{}' not found", account_name)),
    };
    let client = mc.client.lock().await;
    if !client.in_call {
        return Response::fail(&format!("'{}' has no active call", account_name));
    }

    let target_addr = client.server_addr;
    let rtp_port = client.rtp_port_start;
    let codec = mc.codec;
    drop(client);

    match std::fs::read(&wav_path) {
        Ok(data) => match crate::rtp::wav::parse_wav(&data) {
            Ok((info, samples)) => {
                let sample_count = samples.len();
                let sample_rate = info.sample_rate;
                let target = format!("{}:{}", target_addr.ip(), rtp_port)
                    .parse()
                    .unwrap_or(target_addr);

                tokio::spawn(async move {
                    match crate::rtp::send_wav_rtp(
                        &samples,
                        sample_rate,
                        target,
                        0,
                        rtp_port,
                        codec,
                    )
                    .await
                    {
                        Ok(n) => log::info!("Sent {} RTP packets (codec={:?})", n, codec),
                        Err(e) => log::error!("RTP send error: {}", e),
                    }
                });
                Response::ok(&format!(
                    "Playing '{}' ({} samples, {}Hz, codec={:?}) to '{}'",
                    wav_path, sample_count, sample_rate, codec, account_name
                ))
            }
            Err(e) => Response::fail(&format!("WAV parse error: {}", e)),
        },
        Err(e) => Response::fail(&format!("Cannot read file '{}': {}", wav_path, e)),
    }
}

// ── Helper ─────────────────────────────────────────────────

/// Validate account field exists and return the ManagedClient
fn get_account<'a>(
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
