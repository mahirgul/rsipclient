//! Background watcher for incoming and in-dialog SIP calls.
//!
//! Listens for incoming INVITE requests (auto-answering or sending 180 Ringing for Web UI prompt),
//! handles incoming BYE requests on both inbound and outbound calls, and manages CANCEL requests.

use crate::config::Account;
use crate::ivr;
use crate::rtp::codec::Codec;
use crate::sip::{utils, SipClient};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Background task: poll SIP socket for incoming INVITEs, in-dialog BYEs, and CANCELs.
pub async fn incoming_call_watcher(
    account_name: String,
    client: Arc<Mutex<SipClient>>,
    codec: Codec,
    account: Account,
    shutdown: Arc<Mutex<bool>>,
    active: Arc<Mutex<bool>>,
    audio_tx: tokio::sync::broadcast::Sender<Vec<i16>>,
) {
    let mut ivr_task: Option<tokio::task::JoinHandle<()>> = None;

    loop {
        if *shutdown.lock().await || !*active.lock().await {
            break;
        }

        // Poll for incoming SIP message
        let msg = {
            let c = client.lock().await;
            c.try_recv(50).await
        };

        let msg = match msg {
            Some(m) => m,
            None => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                continue;
            }
        };

        // 1. Handle incoming BYE (remote party hung up on either an incoming or outgoing call)
        if msg.starts_with("BYE") {
            log::info!("[{}] Remote party hung up (received BYE)", account_name);
            let from_header_val = utils::extract_header(&msg, "From");
            let to_header_val = utils::extract_header(&msg, "To");
            let call_id_val = utils::extract_header(&msg, "Call-ID");
            let cseq_str = utils::extract_header(&msg, "CSeq");
            let via_headers = utils::extract_headers_raw(&msg, "Via");
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
                let branch = utils::extract_param(&msg, "Via", "branch");
                let key = crate::sip::TransactionKey::new(branch, "BYE");
                let is_reliable = c.transport.via_str() != "UDP";
                c.transaction_mgr
                    .record_server_response(key, response.clone(), is_reliable)
                    .await;
                let _ = c
                    .transport
                    .send_to(response.as_bytes(), c.server_addr)
                    .await;
            }

            // Abort any active IVR task
            if let Some(task) = ivr_task.take() {
                task.abort();
            }

            // Clean up call state
            {
                let mut c = client.lock().await;
                let duration_secs = c
                    .call_start_time
                    .map(|t| t.elapsed().as_secs())
                    .unwrap_or(0);
                let cid = c.call_id.clone().unwrap_or_else(|| call_id_val.clone());
                crate::service::logger::record_call_end(&cid, "Completed", duration_secs);
                if let Some(ref rx) = c.rtp_receiver {
                    rx.stop();
                }
                c.clear_dialog_state();
            }
            continue;
        }

        // 2. Handle incoming CANCEL (caller cancelled call before answer)
        if msg.starts_with("CANCEL") {
            log::info!("[{}] Remote party sent CANCEL", account_name);
            let from_header_val = utils::extract_header(&msg, "From");
            let to_header_val = utils::extract_header(&msg, "To");
            let call_id_val = utils::extract_header(&msg, "Call-ID");
            let cseq_str = utils::extract_header(&msg, "CSeq");
            let via_headers = utils::extract_headers_raw(&msg, "Via");
            let via_block = via_headers.join("\r\n");
            let cseq_num = cseq_str
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(1);

            let cancel_response = format!(
                "SIP/2.0 200 OK\r\n\
                 {}\r\n\
                 From: {}\r\n\
                 To: {}\r\n\
                 Call-ID: {}\r\n\
                 CSeq: {} CANCEL\r\n\
                 Content-Length: 0\r\n\
                 \r\n",
                via_block, from_header_val, to_header_val, call_id_val, cseq_num,
            );
            {
                let c = client.lock().await;
                let branch = utils::extract_param(&msg, "Via", "branch");
                let key = crate::sip::TransactionKey::new(branch, "CANCEL");
                let is_reliable = c.transport.via_str() != "UDP";
                c.transaction_mgr
                    .record_server_response(key, cancel_response.clone(), is_reliable)
                    .await;
                let _ = c
                    .transport
                    .send_to(cancel_response.as_bytes(), c.server_addr)
                    .await;
            }

            // Send 487 Request Terminated for the original INVITE (RFC 3261 §9.2)
            let (to_tag, invite_cseq) = {
                let mut c = client.lock().await;
                let tag = c.local_tag.clone();
                let cseq = c.ringing_cseq.unwrap_or(1);
                c.ringing = false;
                c.ringing_from = None;
                c.ringing_call_id = None;
                c.ringing_cseq = None;
                c.ringing_invite_msg = None;
                (tag, cseq)
            };

            let to_487 = if to_header_val.contains(";tag=") {
                to_header_val
            } else {
                format!("{};tag={}", to_header_val, to_tag)
            };

            let resp_487 = format!(
                "SIP/2.0 487 Request Terminated\r\n\
                 {}\r\n\
                 From: {}\r\n\
                 To: {}\r\n\
                 Call-ID: {}\r\n\
                 CSeq: {} INVITE\r\n\
                 Content-Length: 0\r\n\
                 \r\n",
                via_block, from_header_val, to_487, call_id_val, invite_cseq
            );
            {
                let c = client.lock().await;
                let _ = c
                    .transport
                    .send_to(resp_487.as_bytes(), c.server_addr)
                    .await;
            }
            crate::service::logger::record_call_end(&call_id_val, "Cancelled", 0);
            continue;
        }

        // 3. Handle incoming INVITE
        if msg.starts_with("INVITE") {
            log::info!("[{}] Incoming INVITE!", account_name);
            log::debug!("--- INCOMING ---\n{}", msg);

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

            // Check if already in a call on this account -> send 486 Busy Here
            let is_busy = {
                let c = client.lock().await;
                c.in_call
            };
            if is_busy {
                log::warn!(
                    "[{}] Account is already in a call, rejecting new INVITE with 486 Busy Here",
                    account_name
                );
                let to_formatted = if to_header_val.contains(";tag=") {
                    to_header_val
                } else {
                    let c = client.lock().await;
                    format!("{};tag={}", to_header_val, c.local_tag)
                };
                let busy_resp = format!(
                    "SIP/2.0 486 Busy Here\r\n\
                     {}\r\n\
                     From: {}\r\n\
                     To: {}\r\n\
                     Call-ID: {}\r\n\
                     CSeq: {} INVITE\r\n\
                     Content-Length: 0\r\n\
                     \r\n",
                    via_block, from_header_val, to_formatted, call_id, cseq
                );
                let c = client.lock().await;
                let _ = c
                    .transport
                    .send_to(busy_resp.as_bytes(), c.server_addr)
                    .await;
                continue;
            }

            let auto_answer = account.auto_answer.unwrap_or(false);

            if auto_answer {
                // Auto-Answer: record call start and answer with 200 OK + SDP
                crate::service::logger::record_call_start(
                    &call_id,
                    &account_name,
                    &remote_uri.clone().unwrap_or_default(),
                    "IN",
                );

                let mut c = client.lock().await;
                let answered = c.answer_incoming(&msg, codec, Some(audio_tx.clone())).await;
                drop(c);

                match answered {
                    Ok(true) => {
                        log::info!(
                            "[{}] Incoming call auto-answered successfully",
                            account_name
                        );

                        // Run IVR in background if configured
                        if let Some(ivr_config) = ivr::build_ivr_config(&account) {
                            let c = client.lock().await;
                            let remote_addr = c.remote_rtp_addr;
                            let receiver_opt = c.rtp_receiver.clone();
                            drop(c);

                            if let (Some(remote_addr), Some(receiver)) = (remote_addr, receiver_opt)
                            {
                                log::info!("[{}] Starting IVR session in background", account_name);
                                let session = ivr::IvrSession::new(ivr_config, codec);
                                let client_clone = client.clone();
                                let name_clone = account_name.clone();
                                ivr_task = Some(tokio::spawn(async move {
                                    if let Err(e) =
                                        session.run(&client_clone, remote_addr, &receiver).await
                                    {
                                        log::error!("[{}] IVR error: {}", name_clone, e);
                                    }
                                }));
                            }
                        }
                    }
                    Ok(false) => {
                        log::error!("[{}] Failed to auto-answer incoming call", account_name);
                    }
                    Err(e) => {
                        log::error!("[{}] Error auto-answering call: {}", account_name, e);
                    }
                }
            } else {
                // Manual Answer Mode: Send 180 Ringing (RFC 3261 §13.3.1.1) and notify Web UI
                log::info!(
                    "[{}] Incoming call from {:?}, sending 180 Ringing and awaiting dashboard answer",
                    account_name,
                    remote_uri
                );
                crate::service::logger::record_call_start(
                    &call_id,
                    &account_name,
                    &remote_uri.clone().unwrap_or_default(),
                    "IN",
                );

                let (local_tag, local_addr_str, username, scheme) = {
                    let c = client.lock().await;
                    let via_transport = c.transport.via_str();
                    let s = if via_transport.to_uppercase() == "TLS" {
                        "sips"
                    } else {
                        "sip"
                    };
                    (
                        c.local_tag.clone(),
                        c.local_addr_str(),
                        c.username.clone(),
                        s,
                    )
                };

                let to_formatted = if to_header_val.contains(";tag=") {
                    to_header_val
                } else {
                    format!("{};tag={}", to_header_val, local_tag)
                };

                let ringing_resp = format!(
                    "SIP/2.0 180 Ringing\r\n\
                     {}\r\n\
                     From: {}\r\n\
                     To: {}\r\n\
                     Call-ID: {}\r\n\
                     CSeq: {} INVITE\r\n\
                     Contact: <{}:{}@{}>\r\n\
                     Content-Length: 0\r\n\
                     \r\n",
                    via_block,
                    from_header_val,
                    to_formatted,
                    call_id,
                    cseq,
                    scheme,
                    username,
                    local_addr_str
                );

                {
                    let mut c = client.lock().await;
                    let branch = utils::extract_param(&msg, "Via", "branch");
                    let key = crate::sip::TransactionKey::new(branch, "INVITE");
                    let is_reliable = c.transport.via_str() != "UDP";
                    c.transaction_mgr
                        .record_server_response(key, ringing_resp.clone(), is_reliable)
                        .await;
                    let _ = c
                        .transport
                        .send_to(ringing_resp.as_bytes(), c.server_addr)
                        .await;

                    // Set ringing state for the dashboard
                    c.ringing = true;
                    c.ringing_from = remote_uri;
                    c.ringing_call_id = Some(call_id);
                    c.ringing_cseq = Some(cseq);
                    c.ringing_invite_msg = Some(msg);
                }
            }
        }
    }
}

/// Parse RTP connection address from SDP body, return SocketAddr (RFC 4566)
pub fn parse_sdp_connection(msg: &str) -> Option<SocketAddr> {
    crate::sip::sdp::parse_sdp_connection(msg)
}
