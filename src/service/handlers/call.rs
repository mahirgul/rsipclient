use crate::ipc::{Request, Response};
use crate::service::ManagedClient;
use std::collections::HashMap;

pub async fn handle_call(req: &Request, clients: &HashMap<String, ManagedClient>) -> Response {
    let target = match &req.target {
        Some(t) => t.clone(),
        None => return Response::fail("'call' requires 'target' field"),
    };
    let account_name = super::get_account(req, "call", clients);
    let mc = match account_name {
        Ok(name) => name,
        Err(resp) => return resp,
    };
    let mut client = mc.client.lock().await;
    let codec = mc.codec;
    let audio_tx = mc.audio_tx.clone();
    match client.invite(&target).await {
        Ok(true) => {
            if let Some(ref rx) = client.rtp_receiver {
                rx.start(codec, Some(audio_tx));
            }
            if client.settings.session_timers {
                crate::service::managed_client::spawn_session_refresher(mc.client.clone(), 1800);
            }
            Response::ok(&format!(
                "'{}' calling {} - established",
                req.account.as_deref().unwrap(),
                target
            ))
        }
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

pub async fn handle_hangup(req: &Request, clients: &HashMap<String, ManagedClient>) -> Response {
    let account_name = super::get_account(req, "hangup", clients);
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

pub async fn handle_cancel(req: &Request, clients: &HashMap<String, ManagedClient>) -> Response {
    let account_name = super::get_account(req, "cancel", clients);
    let mc = match account_name {
        Ok(name) => name,
        Err(resp) => return resp,
    };
    let mut client = mc.client.lock().await;
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

pub async fn handle_play(req: &Request, clients: &HashMap<String, ManagedClient>) -> Response {
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

    let target = match client.remote_rtp_addr {
        Some(addr) => addr,
        None => {
            return Response::fail(&format!(
                "No remote RTP address for '{}'; call may not be fully established",
                account_name
            ));
        }
    };
    let rtp_port = client.rtp_port_start;
    let codec = mc.codec;
    let socket_opt = client.rtp_receiver.as_ref().map(|r| r.socket());
    drop(client);

    match crate::rtp::play_wav_file(&wav_path, socket_opt, target, codec, rtp_port).await {
        Ok((sample_count, sample_rate)) => Response::ok(&format!(
            "Playing '{}' ({} samples, {}Hz, codec={:?}) to '{}'",
            wav_path, sample_count, sample_rate, codec, account_name
        )),
        Err(e) => Response::fail(&format!("Play WAV error: {}", e)),
    }
}

pub async fn handle_hold(req: &Request, clients: &HashMap<String, ManagedClient>) -> Response {
    let account_name = super::get_account(req, "hold", clients);
    let mc = match account_name {
        Ok(name) => name,
        Err(resp) => return resp,
    };
    let mut client = mc.client.lock().await;
    match client.hold().await {
        Ok(true) => Response::ok(&format!(
            "'{}' call put on hold",
            req.account.as_deref().unwrap()
        )),
        Ok(false) => Response::fail(&format!(
            "'{}' hold failed or no active call",
            req.account.as_deref().unwrap()
        )),
        Err(e) => Response::fail(&format!(
            "'{}' error: {}",
            req.account.as_deref().unwrap(),
            e
        )),
    }
}

pub async fn handle_resume(req: &Request, clients: &HashMap<String, ManagedClient>) -> Response {
    let account_name = super::get_account(req, "resume", clients);
    let mc = match account_name {
        Ok(name) => name,
        Err(resp) => return resp,
    };
    let mut client = mc.client.lock().await;
    match client.resume().await {
        Ok(true) => Response::ok(&format!(
            "'{}' call resumed",
            req.account.as_deref().unwrap()
        )),
        Ok(false) => Response::fail(&format!(
            "'{}' resume failed or no active call",
            req.account.as_deref().unwrap()
        )),
        Err(e) => Response::fail(&format!(
            "'{}' error: {}",
            req.account.as_deref().unwrap(),
            e
        )),
    }
}

pub async fn handle_transfer(req: &Request, clients: &HashMap<String, ManagedClient>) -> Response {
    let target = match &req.target {
        Some(t) => t.clone(),
        None => return Response::fail("'transfer' requires 'target' field"),
    };
    let account_name = super::get_account(req, "transfer", clients);
    let mc = match account_name {
        Ok(name) => name,
        Err(resp) => return resp,
    };
    let mut client = mc.client.lock().await;
    match client.transfer(&target).await {
        Ok(true) => Response::ok(&format!(
            "'{}' call transfer to {} initiated",
            req.account.as_deref().unwrap(),
            target
        )),
        Ok(false) => Response::fail(&format!(
            "'{}' transfer failed or no active call",
            req.account.as_deref().unwrap()
        )),
        Err(e) => Response::fail(&format!(
            "'{}' error: {}",
            req.account.as_deref().unwrap(),
            e
        )),
    }
}

pub async fn handle_dtmf(req: &Request, clients: &HashMap<String, ManagedClient>) -> Response {
    let digits = match &req.target {
        Some(d) => d.clone(),
        None => return Response::fail("'dtmf' requires 'target' (digits) field"),
    };
    let account_name = super::get_account(req, "dtmf", clients);
    let mc = match account_name {
        Ok(name) => name,
        Err(resp) => return resp,
    };
    let mut client = mc.client.lock().await;
    match client.send_dtmf(&digits).await {
        Ok(true) => Response::ok(&format!(
            "'{}' sent DTMF digits: {}",
            req.account.as_deref().unwrap(),
            digits
        )),
        Ok(false) => Response::fail(&format!(
            "'{}' DTMF send failed or no active call",
            req.account.as_deref().unwrap()
        )),
        Err(e) => Response::fail(&format!(
            "'{}' error: {}",
            req.account.as_deref().unwrap(),
            e
        )),
    }
}
