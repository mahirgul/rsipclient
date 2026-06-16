//! Background watcher for incoming SIP calls.
//!
//! Listens for incoming INVITE requests, answers them automatically, and runs the IVR media session.

use crate::config::Account;
use crate::ivr;
use crate::rtp::codec::Codec;
use crate::rtp::receiver::RtpReceiver;
use crate::sip::{sdp, utils, SipClient};
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
    audio_tx: tokio::sync::broadcast::Sender<Vec<i16>>,
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
        let from_header_val = utils::extract_header(&msg, "From");
        let to_header_val = utils::extract_header(&msg, "To");
        let remote_uri = utils::extract_uri(&from_header_val);
        let call_id = utils::extract_header(&msg, "Call-ID");
        let cseq_str = utils::extract_header(&msg, "CSeq");
        let cseq: u32 = cseq_str
            .split_whitespace()
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let via_headers = utils::extract_headers_raw(&msg, "Via");
        let via_block = via_headers.join("\r\n");

        // Parse remote RTP addr from SDP
        let remote_rtp = parse_sdp_connection(&msg);

        // Find and bind a free RTP receiver in the range
        let (rtp_port_start, rtp_port_end) = {
            let c = client.lock().await;
            (c.rtp_port_start, c.rtp_port_end)
        };
        let (receiver, bound_rtp_port) =
            match RtpReceiver::bind_range(rtp_port_start, rtp_port_end).await {
                Ok(r) => r,
                Err(e) => {
                    log::error!(
                        "[{}] Failed to bind RTP receiver in range {}-{}: {}",
                        account_name,
                        rtp_port_start,
                        rtp_port_end,
                        e
                    );
                    let response = format!(
                        "SIP/2.0 503 Service Unavailable\r\n\
                         {}\r\n\
                         From: {}\r\n\
                         To: {}\r\n\
                         Call-ID: {}\r\n\
                         CSeq: {} INVITE\r\n\
                         Content-Length: 0\r\n\
                         \r\n",
                        via_block, from_header_val, to_header_val, call_id, cseq,
                    );
                    let c = client.lock().await;
                    let _ = c
                        .transport
                        .send_to(response.as_bytes(), c.server_addr)
                        .await;
                    continue;
                }
            };

        // Auto-answer: build 200 OK with SDP
        let response = {
            let c = client.lock().await;
            let local_ip = c.local_addr.ip().to_string();
            let sdp_body = sdp::build_sdp_default(&c.username, &local_ip, bound_rtp_port);
            let sdp_len = sdp_body.len();
            let via_transport = c.transport.via_str();
            let scheme = if via_transport.to_uppercase() == "TLS" {
                "sips"
            } else {
                "sip"
            };

            format!(
                "SIP/2.0 200 OK\r\n\
                 {}\r\n\
                 From: {}\r\n\
                 To: {};tag={}\r\n\
                 Call-ID: {}\r\n\
                 CSeq: {} INVITE\r\n\
                 Contact: <{}:{}@{}>\r\n\
                 Content-Type: application/sdp\r\n\
                 Content-Length: {}\r\n\
                 \r\n\
                 {}",
                via_block,
                from_header_val,
                to_header_val,
                c.local_tag,
                call_id,
                cseq,
                scheme,
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
            log::warn!("[{}] No ACK received, skipping call setup", account_name);
            continue;
        }

        // Start RTP receiver
        receiver.start(codec, Some(audio_tx.clone()));

        // Mark as in-call
        {
            let mut c = client.lock().await;
            c.in_call = true;
            c.call_id = Some(call_id.clone());
            c.invite_cseq = Some(cseq);
            c.remote_tag = Some(from_tag.clone());
            c.remote_rtp_addr = remote_rtp;
            c.remote_uri = remote_uri;
            c.rtp_receiver = Some(receiver.clone());
        }

        // Run IVR in background if configured
        let ivr_task = if let Some(ivr_config) = ivr::build_ivr_config(&account) {
            if let Some(remote_addr) = remote_rtp {
                log::info!("[{}] Starting IVR session in background", account_name);
                let session = ivr::IvrSession::new(ivr_config, codec);
                let client_clone = client.clone();
                let receiver_clone = receiver.clone();
                let name_clone = account_name.clone();
                Some(tokio::spawn(async move {
                    if let Err(e) = session
                        .run(&client_clone, remote_addr, &receiver_clone)
                        .await
                    {
                        log::error!("[{}] IVR error: {}", name_clone, e);
                    }
                }))
            } else {
                log::warn!("[{}] No RTP address in SDP, skipping IVR", account_name);
                None
            }
        } else {
            None
        };

        // Keep waiting while the call is active (checking for BYE from remote)
        loop {
            let is_active = {
                let c = client.lock().await;
                c.in_call
            };
            if !is_active {
                break;
            }

            // Poll for incoming SIP messages (like BYE)
            let msg = {
                let c = client.lock().await;
                c.try_recv(200).await
            };

            if let Some(m) = msg {
                if m.starts_with("BYE") {
                    log::info!("[{}] Remote party hung up (received BYE)", account_name);
                    let from_header_val = utils::extract_header(&m, "From");
                    let to_header_val = utils::extract_header(&m, "To");
                    let call_id_val = utils::extract_header(&m, "Call-ID");
                    let cseq_str = utils::extract_header(&m, "CSeq");
                    let via_headers = utils::extract_headers_raw(&m, "Via");
                    let via_block = via_headers.join("\r\n");

                    let cseq_num = cseq_str
                        .split_whitespace()
                        .next()
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(1);

                    let response = format!(
                        "SIP/2.0 200 OK\r\n\
                         {}\r\n\
                         From: {}\r\n\
                         To: {}\r\n\
                         Call-ID: {}\r\n\
                         CSeq: {} BYE\r\n\
                         Content-Length: 0\r\n\
                         \r\n",
                        via_block, from_header_val, to_header_val, call_id_val, cseq_num,
                    );

                    {
                        let c = client.lock().await;
                        let _ = c
                            .transport
                            .send_to(response.as_bytes(), c.server_addr)
                            .await;
                    }

                    let mut c = client.lock().await;
                    c.in_call = false;
                    break;
                }
            }

            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }

        // Wait for the IVR task to finish (e.g. saving voicemail WAV files)
        if let Some(mut task) = ivr_task {
            match tokio::time::timeout(std::time::Duration::from_secs(2), &mut task).await {
                Ok(res) => {
                    if let Err(e) = res {
                        log::error!("[{}] IVR task joined with error: {:?}", account_name, e);
                    }
                }
                Err(_) => {
                    log::warn!(
                        "[{}] IVR task did not finish in time, aborting",
                        account_name
                    );
                    task.abort();
                }
            }
        }

        // Cleanup call state
        {
            let mut c = client.lock().await;
            // Stop RTP receiver to prevent resource leak
            if let Some(ref rx) = c.rtp_receiver {
                rx.stop();
            }
            c.in_call = false;
            c.call_id = None;
            c.invite_cseq = None;
            c.remote_tag = None;
            c.remote_rtp_addr = None;
            c.remote_uri = None;
            c.rtp_receiver = None;
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
