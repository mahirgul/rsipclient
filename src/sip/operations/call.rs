//! SIP call establishment and termination operations.
//!
//! This file implements call setup (INVITE, ACK), call termination (BYE, CANCEL),
//! and in-call features like DTMF digit transmission.

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
        let branch = self.new_branch();
        let cseq = self.next_cseq().await;
        let local = self.local_addr_str();
        let sdp_body = sdp::build_sdp_default(
            &self.username,
            &self.local_addr.ip().to_string(),
            bound_rtp_port,
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

        let resp = self.send(&msg).await?;
        let status = utils::parse_status_code(&resp)?;

        // Handle 401/407 auth challenge for INVITE
        if (status == 401 || status == 407) && self.auth_method == crate::sip::AuthMethod::Md5 {
            let (realm, nonce) = utils::extract_auth_params(&resp)
                .context("Cannot extract WWW-Authenticate params for INVITE")?;

            let auth_cseq = self.next_cseq().await;
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
                &realm,
                &nonce,
                &self.settings,
                self.transport.via_str(),
            );

            let resp2 = self.send(&auth_msg).await?;
            let status2 = utils::parse_status_code(&resp2)?;

            let mut final_status2 = status2;
            let mut final_resp2 = resp2.clone();
            let mut final_tag2 = utils::extract_to_tag(&resp2);

            while (100..200).contains(&final_status2) {
                log::info!(
                    "Got provisional response {} (auth INVITE) — waiting for final...",
                    final_status2
                );
                final_resp2 = match self.recv_extra(30000).await {
                    Ok(r) => r,
                    Err(e) => {
                        log::error!("Error waiting for final response (auth INVITE): {}", e);
                        self.remote_uri = None;
                        return Ok(false);
                    }
                };
                final_status2 = match utils::parse_status_code(&final_resp2) {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("Error parsing status (auth INVITE): {}", e);
                        self.remote_uri = None;
                        return Ok(false);
                    }
                };
                if let Some(t) = utils::extract_to_tag(&final_resp2) {
                    final_tag2 = Some(t);
                }
            }

            if final_status2 == 200 {
                self.call_id = Some(call_id.clone());
                self.invite_cseq = Some(auth_cseq);
                self.remote_tag = final_tag2;
                self.remote_rtp_addr = crate::service::watcher::parse_sdp_connection(&final_resp2);
                self.rtp_receiver = Some(receiver);
                self.in_call = true;
                self.send_ack(target_uri, &local, &call_id, auth_cseq)
                    .await?;
                log::info!(
                    "Call established (with INVITE auth)! Remote RTP: {:?}",
                    self.remote_rtp_addr
                );
                return Ok(true);
            }

            log::error!("Auth INVITE failed (status={})", final_status2);
            self.remote_uri = None;
            return Ok(false);
        }

        // Wait for final response if we received provisional (1xx) responses
        let mut final_status = status;
        let mut final_resp = resp.clone();
        let mut final_tag = utils::extract_to_tag(&resp);

        while (100..200).contains(&final_status) {
            log::info!(
                "Got provisional response {} — waiting for final...",
                final_status
            );
            final_resp = match self.recv_extra(30000).await {
                Ok(r) => r,
                Err(e) => {
                    log::error!("Error waiting for final response: {}", e);
                    self.remote_uri = None;
                    return Ok(false);
                }
            };
            final_status = match utils::parse_status_code(&final_resp) {
                Ok(s) => s,
                Err(e) => {
                    log::error!("Error parsing status: {}", e);
                    self.remote_uri = None;
                    return Ok(false);
                }
            };
            if let Some(t) = utils::extract_to_tag(&final_resp) {
                final_tag = Some(t);
            }
        }

        if final_status == 200 {
            self.call_id = Some(call_id.clone());
            self.invite_cseq = Some(cseq);
            self.remote_tag = final_tag;
            self.in_call = true;
            self.remote_rtp_addr = crate::service::watcher::parse_sdp_connection(&final_resp);
            self.rtp_receiver = Some(receiver);
            self.send_ack(target_uri, &local, &call_id, cseq).await?;
            log::info!("Call established! Remote RTP: {:?}", self.remote_rtp_addr);
            return Ok(true);
        }

        log::error!("Call failed (status={})", final_status);
        self.in_call = false;
        self.call_id = None;
        self.invite_cseq = None;
        self.remote_tag = None;
        self.remote_uri = None;
        self.remote_rtp_addr = None;
        self.rtp_receiver = None;
        Ok(false)
    }

    /// ACK helper — sent after 200 OK to confirm call setup
    async fn send_ack(
        &self,
        target_uri: &str,
        local_addr_str: &str,
        call_id: &str,
        cseq: u32,
    ) -> Result<()> {
        let ack = build_ack(
            target_uri,
            &self.username,
            &self.domain,
            local_addr_str,
            &self.local_tag,
            self.remote_tag.as_deref().unwrap_or(""),
            call_id,
            cseq,
            &self.new_branch(),
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

        let call_id = self.call_id.as_ref().context("No call_id")?;
        let remote_tag = self.remote_tag.as_ref().context("No remote_tag")?;
        let remote_uri = self.remote_uri.as_ref().context("No remote_uri")?;
        let local = self.local_addr_str();

        let msg = build_bye(
            &self.username,
            &self.domain,
            remote_uri,
            &local,
            &self.local_tag,
            remote_tag,
            call_id,
            self.next_cseq().await,
            &self.new_branch(),
            &self.settings,
            self.transport.via_str(),
        );

        let resp = self.send(&msg).await?;
        let status = utils::parse_status_code(&resp)?;

        if let Some(ref rx) = self.rtp_receiver {
            rx.stop();
        }

        if status == 200 {
            log::info!("Call ended successfully");
        } else {
            log::error!("Failed to end call cleanly (status={})", status);
        }
        self.in_call = false;
        self.held = false;
        self.call_id = None;
        self.invite_cseq = None;
        self.remote_tag = None;
        self.remote_rtp_addr = None;
        self.remote_uri = None;
        self.rtp_receiver = None;
        Ok(status == 200)
    }

    /// Send CANCEL for the current INVITE transaction.
    /// Uses the same CSeq as the INVITE (RFC 3261 §9.1).
    pub async fn cancel(&mut self) -> Result<bool> {
        let call_id = self.call_id.as_ref().context("No active call")?;
        let remote_uri = self.remote_uri.as_ref().context("No remote_uri")?;
        let invite_cseq = self.invite_cseq.context("No INVITE CSeq stored")?;
        let local = self.local_addr_str();

        let msg = build_cancel(
            &self.username,
            &self.domain,
            remote_uri,
            &local,
            &self.local_tag,
            call_id,
            invite_cseq,
            &self.new_branch(),
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
            self.in_call = false;
            self.call_id = None;
            self.invite_cseq = None;
            self.remote_tag = None;
            self.remote_rtp_addr = None;
            self.remote_uri = None;
            self.rtp_receiver = None;
        }
        Ok(success)
    }

    /// Send DTMF digits on the active call.
    pub async fn send_dtmf(&mut self, digits: &str) -> Result<bool> {
        if !self.in_call {
            log::warn!("No active call to send DTMF");
            return Ok(false);
        }
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

        Ok(true)
    }
}
