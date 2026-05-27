//! Call watcher module - handles incoming call detection, auto-answering, and IVR execution.

use crate::config::Account;
use crate::rtp::codec::Codec;
use crate::rtp::receiver::RtpReceiver;
use crate::sip::{sdp, utils, SipClient};
use crate::ivr;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Background task: poll SIP socket for incoming INVITEs, auto-answer, run IVR.
pub async fn incoming_call_watcher(
    account_name: String,
    client: Arc<Mutex<SipClient>>,
    codec: Codec,
    account: Account,
    shutdown: Arc<Mutex<bool>>,
    active: Arc<Mutex<bool>>,
) {
    loop {
        if *shutdown.lock().await || !*active.lock().await {
            break;
        }

        // Poll for incoming SIP message
        let msg = {
            let c = client.lock().await;
            c.try_recv(200).await
        };

        let msg = match msg {
            Some(m) => m,
            None => {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                continue;
            }
        };

        // Only handle INVITE
        if !msg.starts_with("INVITE") {
            continue;
        }

        log::info!("[{}] Incoming INVITE!", account_name);
        log::debug!("--- INCOMING ---\n{}", msg);

        // Extract Call-ID, From tag, To tag, Contact, and remote RTP from SDP
        let from_tag = utils::extract_param(&msg, "From", "tag");
        let call_id = utils::extract_header(&msg, "Call-ID");
        let cseq_str = utils::extract_header(&msg, "CSeq");
        let cseq: u32 = cseq_str
            .split_whitespace()
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let via_branch = utils::extract_param(&msg, "Via", "branch");

        // Parse remote RTP addr from SDP
        let remote_rtp = parse_sdp_connection(&msg);

        // Auto-answer: build 200 OK with SDP
        let response = {
            let c = client.lock().await;
            let local_ip = c.local_addr.ip().to_string();
            let sdp_body = sdp::build_sdp_default(&c.username, &local_ip, c.rtp_port_start);
            let sdp_len = sdp_body.len();

            format!(
                "SIP/2.0 200 OK\r\n\
                 Via: SIP/2.0/UDP {};branch={}\r\n\
                 From: <sip:{}@{}>;tag={}\r\n\
                 To: <sip:{}@{}>;tag={}\r\n\
                 Call-ID: {}\r\n\
                 CSeq: {} INVITE\r\n\
                 Contact: <sip:{}@{}>\r\n\
                 Content-Type: application/sdp\r\n\
                 Content-Length: {}\r\n\
                 \r\n\
                 {}",
                c.local_addr_str(),
                via_branch,
                c.username,
                c.domain,
                from_tag,
                c.username,
                c.domain,
                c.local_tag,
                call_id,
                cseq,
                c.username,
                c.local_addr_str(),
                sdp_len,
                sdp_body,
            )
        };

        // Send 200 OK
        {
            let c = client.lock().await;
            log::debug!("--- SEND 200 OK ---\n{}", response);
            let _ = c
                .transport
                .send_to(response.as_bytes(), c.server_addr)
                .await;
        }

        // Wait for ACK
        let ack_received = {
            let c = client.lock().await;
            match c.recv_extra(5000).await {
                Ok(ack) => ack.starts_with("ACK"),
                Err(_) => false,
            }
        };

        if !ack_received {
            log::warn!("[{}] No ACK received, skipping IVR", account_name);
            continue;
        }

        // Start RTP receiver
        let rtp_port = {
            let c = client.lock().await;
            c.rtp_port_start
        };
        let receiver = match RtpReceiver::bind(rtp_port).await {
            Ok(r) => r,
            Err(e) => {
                log::error!("[{}] Failed to bind RTP receiver: {}", e, account_name);
                continue;
            }
        };
        receiver.start();

        // Mark as in-call
        {
            let mut c = client.lock().await;
            c.in_call = true;
            c.call_id = Some(call_id.clone());
            c.remote_tag = Some(from_tag.clone());
        }

        // Run IVR if configured
        if let Some(ivr_config) = ivr::build_ivr_config(&account) {
            let remote_addr = match remote_rtp {
                Some(addr) => addr,
                None => {
                    log::warn!("[{}] No RTP address in SDP", account_name);
                    continue;
                }
            };

            log::info!("[{}] Starting IVR session", account_name);
            let session = ivr::IvrSession::new(ivr_config, codec);
            if let Err(e) = session.run(&client, remote_addr, &receiver).await {
                log::error!("[{}] IVR error: {}", e, account_name);
            }
        }

        // Cleanup call state
        {
            let mut c = client.lock().await;
            c.in_call = false;
            c.call_id = None;
            c.remote_tag = None;
        }
    }
}

/// Parse the first `c=` line from SDP body, return SocketAddr
pub fn parse_sdp_connection(msg: &str) -> Option<SocketAddr> {
    // Find c= line
    let c_line = msg.lines().find(|l| l.starts_with("c="))?;
    // c=IN IP4 192.168.1.1
    let parts: Vec<&str> = c_line.split_whitespace().collect();
    let ip = parts.get(2)?;

    // Find m= line for port
    let m_line = msg.lines().find(|l| l.starts_with("m=audio"))?;
    let m_parts: Vec<&str> = m_line.split_whitespace().collect();
    let port: u16 = m_parts.get(1)?.parse().ok()?;

    format!("{}:{}", ip, port).parse().ok()
}

/// Background task: monitor registration state of a client and refresh or retry accordingly.
pub async fn registration_watcher(
    account_name: String,
    client: Arc<Mutex<SipClient>>,
    active: Arc<Mutex<bool>>,
    should_register: Arc<Mutex<bool>>,
    register_expiry: u32,
    retry_interval: u32,
    shutdown: Arc<Mutex<bool>>,
) {
    let mut last_register_time: Option<std::time::Instant> = None;
    let mut is_currently_registered = false;

    loop {
        if *shutdown.lock().await || !*active.lock().await {
            break;
        }

        let want_register = *should_register.lock().await;

        if want_register {
            let now = std::time::Instant::now();
            let need_retry_or_refresh = match last_register_time {
                None => true, // never tried
                Some(last_time) => {
                    if is_currently_registered {
                        // Refresh registration at half of expiry time
                        let refresh_duration = std::time::Duration::from_secs((register_expiry / 2).max(10) as u64);
                        now.duration_since(last_time) >= refresh_duration
                    } else {
                        // Retry registration after retry_interval
                        let retry_duration = std::time::Duration::from_secs(retry_interval.max(5) as u64);
                        now.duration_since(last_time) >= retry_duration
                    }
                }
            };

            if need_retry_or_refresh {
                log::info!("[{}] Attempting registration...", account_name);
                let reg_res = {
                    let c = client.lock().await;
                    c.register().await
                };

                last_register_time = Some(std::time::Instant::now());
                match reg_res {
                    Ok(true) => {
                        log::info!("[{}] Registration successful", account_name);
                        is_currently_registered = true;
                    }
                    Ok(false) => {
                        log::warn!("[{}] Registration failed, will retry", account_name);
                        is_currently_registered = false;
                        // Force register flag in client to false
                        let c = client.lock().await;
                        *c.registered.lock().await = false;
                    }
                    Err(e) => {
                        log::error!("[{}] Registration error: {}, will retry", account_name, e);
                        is_currently_registered = false;
                        // Force register flag in client to false
                        let c = client.lock().await;
                        *c.registered.lock().await = false;
                    }
                }
            }
        } else {
            // If they don't want to be registered, but are registered, unregister them
            let is_reg = {
                let c = client.lock().await;
                let reg_val = *c.registered.lock().await;
                reg_val
            };
            if is_reg {
                log::info!("[{}] Unregistering as requested...", account_name);
                let reg_res = {
                    let c = client.lock().await;
                    c.unregister().await
                };
                if let Err(e) = reg_res {
                    log::error!("[{}] Unregistration error: {}", account_name, e);
                }
                is_currently_registered = false;
                last_register_time = None;
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
