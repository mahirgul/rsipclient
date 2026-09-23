//! SIP call establishment and termination operations.
//!
//! This file implements call setup (INVITE, ACK), call termination (BYE, CANCEL),
//! and in-call features like DTMF digit transmission.

use crate::rtp::codec::Codec;
use crate::sip::client::SipClient;
use crate::sip::messages::{
    build_ack, build_bye, build_cancel, build_invite, build_invite_with_auth,
};
use crate::sip::sdp;
use crate::sip::utils;
use anyhow::{Context, Result};

impl SipClient {
    /// Send INVITE to establish a call. Returns true if call is set up.
    /// Handles 401/407 authentication challenges.
    pub async fn invite(&mut self, target_uri: &str) -> Result<bool> {
        utils::validate_header_value(target_uri, "call target")?;
        let formatted_uri = if target_uri.starts_with("sip:") || target_uri.starts_with("sips:") {
            target_uri.to_string()
        } else if target_uri.contains('@') {
            format!("sip:{}", target_uri)
        } else {
            format!("sip:{}@{}", target_uri, self.domain)
        };
        let target_uri = &formatted_uri;
        self.remote_uri = Some(target_uri.to_string());

        // Find and bind a free RTP port in our range
        let (receiver, bound_rtp_port) =
            crate::rtp::receiver::RtpReceiver::bind_range(self.rtp_port_start, self.rtp_port_end)
                .await?;

        let call_id = self.new_call_id();
        crate::service::logger::record_call_start(&call_id, &self.username, target_uri, "OUT");
        let branch = self.new_branch();
        let cseq = self.next_cseq().await;
        let local = self.local_addr_str();
        let configured_codec = Codec::from_str(&self.codec).unwrap_or(Codec::Pcmu);
        let sdp_body = sdp::build_sdp_single(
            &self.username,
            &self.local_addr.ip().to_string(),
            bound_rtp_port,
            configured_codec,
        );

        let msg = build_invite(
            target_uri,
            &self.username,
            &self.domain,
            &local,
            &self.local_tag,
            &branch,
            &call_id,
            cseq,
            &sdp_body,
            &self.settings,
            self.transport.via_str(),
        );

        // The transaction layer (execute_invite) already retransmits per Timer
        // A and waits internally for a final (non-1xx) response or a Timer B
        // timeout, so `resp` here is never provisional. A send error (e.g.
        // Timer B expiry) must still clear the `remote_uri` set above so a
        // failed call doesn't leave stale call state behind.
        let resp = match self.send(&msg).await {
            Ok(r) => r,
            Err(e) => {
                log::error!("INVITE failed: {}", e);
                crate::service::logger::record_call_end(&call_id, "Failed", 0);
                self.remote_uri = None;
                return Ok(false);
            }
        };
        let status = utils::parse_status_code(&resp)?;

        // Handle 401/407 auth challenge for INVITE
        if (status == 401 || status == 407) && self.auth_method == crate::sip::AuthMethod::Md5 {
            let challenges = utils::extract_all_auth_challenges(&resp);
            if challenges.is_empty() {
                anyhow::bail!("Cannot extract WWW-Authenticate params for INVITE");
            }

            for challenge in challenges {
                let mut auth_cseq = self.next_cseq().await;
                let auth_msg = build_invite_with_auth(
                    target_uri,
                    &self.username,
                    &self.password,
                    &self.domain,
                    &local,
                    &self.local_tag,
                    &self.new_branch(),
                    // Reuse original Call-ID for auth retry (RFC 3261 §22.4)
                    &call_id,
                    auth_cseq,
                    &sdp_body,
                    &challenge,
                    &self.settings,
                    self.transport.via_str(),
                );

                let mut resp2 = match self.send(&auth_msg).await {
                    Ok(r) => r,
                    Err(e) => {
                        log::warn!(
                            "Auth INVITE send failed for realm={} ({}), trying next challenge if available",
                            challenge.realm,
                            e
                        );
                        continue;
                    }
                };
                let mut status2 = utils::parse_status_code(&resp2)?;

                // A proxy whose nonce expired answers stale=true with a fresh one;
                // without this retry the call simply fails and nothing re-places it.
                if let Some(fresh) = super::register::stale_retry_challenge(status2, &resp2) {
                    log::info!("INVITE nonce was stale, retrying with the fresh one");
                    let retry_cseq = self.next_cseq().await;
                    let retry_msg = build_invite_with_auth(
                        target_uri,
                        &self.username,
                        &self.password,
                        &self.domain,
                        &local,
                        &self.local_tag,
                        &self.new_branch(),
                        &call_id,
                        retry_cseq,
                        &sdp_body,
                        &fresh,
                        &self.settings,
                        self.transport.via_str(),
                    );
                    resp2 = match self.send(&retry_msg).await {
                        Ok(r) => r,
                        Err(e) => {
                            log::warn!(
                                "Stale-retry INVITE send failed for realm={} ({}), trying next challenge if available",
                                challenge.realm,
                                e
                            );
                            continue;
                        }
                    };
                    status2 = utils::parse_status_code(&resp2)?;
                    // The ACK and the dialog's CSeq must track the request that was
                    // actually answered.
                    auth_cseq = retry_cseq;
                }

                let final_status2 = status2;
                let final_resp2 = resp2;
                let final_tag2 = utils::extract_to_tag(&final_resp2);

                if final_status2 == 200 {
                    self.call_id = Some(call_id.clone());
                    self.invite_cseq = Some(auth_cseq);
                    self.remote_tag = final_tag2;
                    let mut routes = utils::extract_record_routes(&final_resp2);
                    routes.reverse();
                    self.route_set = routes;
                    if let Some(target) =
                        utils::extract_uri(&utils::extract_header(&final_resp2, "Contact"))
                    {
                        self.remote_target = Some(target);
                    }
                    self.remote_rtp_addr =
                        crate::service::watcher::parse_sdp_connection(&final_resp2);
                    self.rtp_receiver = Some(receiver);
                    self.rtp_port = Some(bound_rtp_port);
                    self.in_call = true;
                    self.call_direction = Some("out".to_string());
                    sdp::warn_codec_mismatch(configured_codec, &final_resp2);
                    self.call_start_time = Some(std::time::Instant::now());
                    if self.settings.session_timers {
                        self.session_expires_secs =
                            utils::parse_session_expires(&final_resp2).map(|se| se.delta_seconds);
                    }
                    self.send_ack(target_uri, &local, &call_id, auth_cseq)
                        .await?;
                    log::info!(
                        "Call established (with INVITE auth realm={})! Remote RTP: {:?}",
                        challenge.realm,
                        self.remote_rtp_addr
                    );
                    crate::service::logger::record_call_connect(&call_id);
                    return Ok(true);
                }

                log::warn!(
                    "Auth INVITE failed for realm={} (status={}), trying next challenge if available",
                    challenge.realm,
                    final_status2
                );
            }

            log::error!("All auth INVITE attempts failed");
            crate::service::logger::record_call_end(&call_id, "Failed", 0);
            self.clear_dialog_state();
            return Ok(false);
        }

        // execute_invite already waited for the final (non-1xx) response.
        let final_status = status;
        let final_resp = resp;
        let final_tag = utils::extract_to_tag(&final_resp);

        if final_status == 200 {
            self.call_id = Some(call_id.clone());
            self.invite_cseq = Some(cseq);
            self.remote_tag = final_tag;
            let mut routes = utils::extract_record_routes(&final_resp);
            routes.reverse();
            self.route_set = routes;
            if let Some(target) = utils::extract_uri(&utils::extract_header(&final_resp, "Contact"))
            {
                self.remote_target = Some(target);
            }
            self.in_call = true;
            self.call_direction = Some("out".to_string());
            self.call_start_time = Some(std::time::Instant::now());
            self.remote_rtp_addr = crate::service::watcher::parse_sdp_connection(&final_resp);
            self.rtp_receiver = Some(receiver);
            self.rtp_port = Some(bound_rtp_port);
            sdp::warn_codec_mismatch(configured_codec, &final_resp);
            if self.settings.session_timers {
                self.session_expires_secs =
                    utils::parse_session_expires(&final_resp).map(|se| se.delta_seconds);
            }
            self.send_ack(target_uri, &local, &call_id, cseq).await?;
            log::info!("Call established! Remote RTP: {:?}", self.remote_rtp_addr);
            crate::service::logger::record_call_connect(&call_id);
            return Ok(true);
        }

        log::error!("Call failed (status={})", final_status);
        crate::service::logger::record_call_end(&call_id, "Failed", 0);
        self.clear_dialog_state();
        Ok(false)
    }

    /// ACK helper — sent after 200 OK to confirm call setup or re-INVITE (RFC 3261 §13.2.2.4 & §14.1)
    pub async fn send_ack(
        &self,
        fallback_target_uri: &str,
        local_addr_str: &str,
        call_id: &str,
        cseq: u32,
    ) -> Result<()> {
        let ack_target = self.remote_target.as_deref().unwrap_or(fallback_target_uri);
        let route_headers = utils::format_route_headers(&self.route_set);
        let ack = build_ack(
            ack_target,
            &self.username,
            &self.domain,
            local_addr_str,
            &self.local_tag,
            self.remote_tag.as_deref().unwrap_or(""),
            call_id,
            cseq,
            &self.new_branch(),
            &route_headers,
            &self.settings,
            self.transport.via_str(),
        );
        self.transport
            .send_to(ack.as_bytes(), self.server_addr)
            .await?;
        Ok(())
    }

    /// Send BYE to end the active call. Cleans up all call state and stops RTP.
    pub async fn bye(&mut self) -> Result<bool> {
        if !self.in_call {
            log::warn!("No active call");
            return Ok(false);
        }

        let call_id = self.call_id.clone().context("No call_id")?;
        let remote_tag = self.remote_tag.as_ref().context("No remote_tag")?;
        let target = self
            .remote_target
            .as_deref()
            .or(self.remote_uri.as_deref())
            .context("No remote_uri or remote_target")?;
        let local = self.local_addr_str();
        let route_headers = utils::format_route_headers(&self.route_set);

        let msg = build_bye(
            &self.username,
            &self.domain,
            target,
            &local,
            &self.local_tag,
            remote_tag,
            &call_id,
            self.next_cseq().await,
            &self.new_branch(),
            &route_headers,
            &self.settings,
            self.transport.via_str(),
        );

        let resp = self.send(&msg).await?;
        let status = utils::parse_status_code(&resp)?;

        if let Some(ref rx) = self.rtp_receiver {
            rx.stop();
        }

        let duration = self
            .call_start_time
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);
        crate::service::logger::record_call_end(&call_id, "Completed", duration);

        if status == 200 {
            log::info!("Call ended successfully");
        } else {
            log::error!("Failed to end call cleanly (status={})", status);
        }
        self.clear_dialog_state();
        Ok(status == 200)
    }

    /// Send CANCEL for the current INVITE transaction.
    /// Uses the same CSeq as the INVITE (RFC 3261 §9.1).
    pub async fn cancel(&mut self) -> Result<bool> {
        let call_id = self.call_id.as_ref().context("No active call")?;
        let remote_uri = self.remote_uri.as_ref().context("No remote_uri")?;
        let invite_cseq = self.invite_cseq.context("No INVITE CSeq stored")?;
        let local = self.local_addr_str();
        let route_headers = utils::format_route_headers(&self.route_set);

        let msg = build_cancel(
            &self.username,
            &self.domain,
            remote_uri,
            &local,
            &self.local_tag,
            call_id,
            invite_cseq,
            &self.new_branch(),
            &route_headers,
            &self.settings,
            self.transport.via_str(),
        );

        let resp = self.send(&msg).await?;
        let status = utils::parse_status_code(&resp)?;
        log::info!("Cancel response: {}", status);

        let success = status == 200 || status == 487;
        if success {
            if let Some(ref rx) = self.rtp_receiver {
                rx.stop();
            }
            crate::service::logger::record_call_end(call_id, "Cancelled", 0);
            self.clear_dialog_state();
        }
        Ok(success)
    }

    /// Send DTMF digits on the active call, honouring the configured `dtmf_mode`.
    pub async fn send_dtmf(&mut self, digits: &str) -> Result<bool> {
        if !self.in_call {
            log::warn!("No active call to send DTMF");
            return Ok(false);
        }

        let mode = self
            .settings
            .dtmf_mode
            .as_deref()
            .unwrap_or("rfc2833")
            .to_lowercase();

        match mode.as_str() {
            "info" => {
                for c in digits.chars() {
                    if let Err(e) = self.send_info_dtmf(c, 250).await {
                        log::error!("INFO DTMF failed for '{}': {}", c, e);
                    }
                }
            }
            "inband" => {
                log::info!("Sending in-band audio DTMF tone(s) via RTP media stream");
                self.send_dtmf_inband(digits).await?;
            }
            _ => {
                self.send_dtmf_rfc2833(digits).await?;
            }
        }

        if let Some(ref cid) = self.call_id {
            crate::service::logger::record_call_dtmf(cid, digits);
        }

        Ok(true)
    }

    /// Send DTMF digits using RFC 2833 telephone-event packets.
    async fn send_dtmf_rfc2833(&self, digits: &str) -> Result<()> {
        let target = self.remote_rtp_addr.context("No remote RTP address")?;
        let rtp_receiver = self
            .rtp_receiver
            .as_ref()
            .context("RTP receiver not started")?;

        let mut seq = 0u16;
        let mut timestamp = 0u32;

        for c in digits.chars() {
            rtp_receiver
                .send_dtmf_digit(c, target, &mut seq, &mut timestamp)
                .await?;
        }
        Ok(())
    }

    /// Send DTMF digits as synthesized in-band audio PCM tones over RTP.
    async fn send_dtmf_inband(&self, digits: &str) -> Result<()> {
        let target = self.remote_rtp_addr.context("No remote RTP address")?;
        let rtp_receiver = self
            .rtp_receiver
            .as_ref()
            .context("RTP receiver not started")?;

        let mut seq = 0u16;
        let mut timestamp = 0u32;
        let codec = crate::rtp::codec::Codec::from_str(&self.codec)
            .unwrap_or(crate::rtp::codec::Codec::Pcmu);

        for c in digits.chars() {
            rtp_receiver
                .send_dtmf_inband(c, target, codec, &mut seq, &mut timestamp)
                .await?;
        }
        Ok(())
    }

    /// Answer an incoming INVITE request with 200 OK and start the RTP receiver (RFC 3261 §13.3.1.4).
    pub async fn answer_incoming(
        &mut self,
        invite_msg: &str,
        codec: Codec,
        audio_tx: Option<tokio::sync::broadcast::Sender<Vec<i16>>>,
    ) -> Result<bool> {
        let from_tag = utils::extract_param(invite_msg, "From", "tag");
        let from_header_val = utils::extract_header(invite_msg, "From");
        let to_header_val = utils::extract_header(invite_msg, "To");
        let remote_uri = utils::extract_uri(&from_header_val);
        let call_id = utils::extract_header(invite_msg, "Call-ID");
        let cseq_str = utils::extract_header(invite_msg, "CSeq");
        let cseq: u32 = cseq_str
            .split_whitespace()
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let via_headers = utils::extract_headers_raw(invite_msg, "Via");
        let via_block = via_headers.join("\r\n");

        let remote_rtp = crate::service::watcher::parse_sdp_connection(invite_msg);
        sdp::warn_codec_mismatch(codec, invite_msg);

        let (receiver, bound_rtp_port) =
            crate::rtp::receiver::RtpReceiver::bind_range(self.rtp_port_start, self.rtp_port_end)
                .await?;

        let record_route_lines = utils::extract_headers_raw(invite_msg, "Record-Route");
        let record_route_block = if record_route_lines.is_empty() {
            String::new()
        } else {
            format!("{}\r\n", record_route_lines.join("\r\n"))
        };
        let remote_contact_target =
            utils::extract_uri(&utils::extract_header(invite_msg, "Contact"));
        let uas_route_set = utils::extract_record_routes(invite_msg);

        let local_ip = self.local_addr.ip().to_string();
        let sdp_body = sdp::build_sdp_single(&self.username, &local_ip, bound_rtp_port, codec);
        let sdp_len = sdp_body.len();
        let via_transport = self.transport.via_str();
        let scheme = if via_transport.to_uppercase() == "TLS" {
            "sips"
        } else {
            "sip"
        };

        let to_formatted = if to_header_val.contains(";tag=") {
            to_header_val.clone()
        } else {
            format!("{};tag={}", to_header_val, self.local_tag)
        };

        let response = format!(
            "SIP/2.0 200 OK\r\n\
             {}\r\n\
             {}\
             From: {}\r\n\
             To: {}\r\n\
             Call-ID: {}\r\n\
             CSeq: {} INVITE\r\n\
             Contact: <{}:{}@{}>\r\n\
             Content-Type: application/sdp\r\n\
             Content-Length: {}\r\n\
             \r\n\
             {}",
            via_block,
            record_route_block,
            from_header_val,
            to_formatted,
            call_id,
            cseq,
            scheme,
            self.username,
            self.local_addr_str(),
            sdp_len,
            sdp_body,
        );

        let branch = utils::extract_param(invite_msg, "Via", "branch");
        let key = crate::sip::TransactionKey::new(branch, "INVITE");
        let is_reliable = self.transport.via_str() != "UDP";
        self.transaction_mgr
            .record_server_response(key, response.clone(), is_reliable)
            .await;
        self.transport
            .send_to(response.as_bytes(), self.server_addr)
            .await?;

        // Start RTP receiver
        receiver.start(codec, audio_tx);

        self.in_call = true;
        self.call_direction = Some("in".to_string());
        self.call_start_time = Some(std::time::Instant::now());
        self.call_id = Some(call_id.clone());
        self.invite_cseq = Some(cseq);
        self.remote_tag = Some(from_tag);
        self.remote_rtp_addr = remote_rtp;
        self.remote_uri = remote_uri;
        self.remote_target = remote_contact_target;
        self.route_set = uas_route_set;
        self.rtp_receiver = Some(receiver);
        self.rtp_port = Some(bound_rtp_port);
        self.ringing = false;
        self.ringing_from = None;
        self.ringing_call_id = None;
        self.ringing_cseq = None;
        self.ringing_invite_msg = None;

        if self.settings.session_timers {
            self.session_expires_secs = utils::parse_session_expires(invite_msg)
                .map(|se| se.delta_seconds)
                .or(Some(1800));
        }

        crate::service::logger::record_call_connect(&call_id);
        log::info!(
            "Incoming call answered! Remote RTP: {:?}",
            self.remote_rtp_addr
        );
        Ok(true)
    }

    /// Reject an incoming ringing call with 486 Busy Here (RFC 3261 §21.4.17).
    pub async fn reject_incoming(&mut self, invite_msg: &str) -> Result<bool> {
        let from_header_val = utils::extract_header(invite_msg, "From");
        let to_header_val = utils::extract_header(invite_msg, "To");
        let call_id = utils::extract_header(invite_msg, "Call-ID");
        let cseq_str = utils::extract_header(invite_msg, "CSeq");
        let via_headers = utils::extract_headers_raw(invite_msg, "Via");
        let via_block = via_headers.join("\r\n");

        let to_formatted = if to_header_val.contains(";tag=") {
            to_header_val
        } else {
            format!("{};tag={}", to_header_val, self.local_tag)
        };

        let response = format!(
            "SIP/2.0 486 Busy Here\r\n\
             {}\r\n\
             From: {}\r\n\
             To: {}\r\n\
             Call-ID: {}\r\n\
             CSeq: {}\r\n\
             Content-Length: 0\r\n\
             \r\n",
            via_block, from_header_val, to_formatted, call_id, cseq_str
        );

        let branch = utils::extract_param(invite_msg, "Via", "branch");
        let key = crate::sip::TransactionKey::new(branch, "INVITE");
        let is_reliable = self.transport.via_str() != "UDP";
        self.transaction_mgr
            .record_server_response(key, response.clone(), is_reliable)
            .await;
        self.transport
            .send_to(response.as_bytes(), self.server_addr)
            .await?;

        crate::service::logger::record_call_end(&call_id, "Rejected", 0);
        self.ringing = false;
        self.ringing_from = None;
        self.ringing_call_id = None;
        self.ringing_cseq = None;
        self.ringing_invite_msg = None;
        log::info!("Incoming call rejected (486 Busy Here sent)");
        Ok(true)
    }
}
