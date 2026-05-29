//! SIP operations — register, invite, bye, cancel
//!
//! These are implemented as methods on SipClient via extension trait pattern,
//! keeping the client.rs struct definition small.

use crate::sip::messages::*;
use crate::sip::sdp;
use crate::sip::utils;
use anyhow::{Context, Result};

use super::client::SipClient;

impl SipClient {
    // ── Register ──────────────────────────────────────────

    /// REGISTER with the server. Returns true on success.
    pub async fn register(&self) -> Result<bool> {
        let local = self.local_addr_str();
        let branch = self.new_branch();
        let call_id = self.new_call_id();
        let cseq = self.next_cseq().await;

        let (msg, _cid, _cs) = build_register(
            &self.username,
            &self.domain,
            &local,
            &self.local_tag,
            &branch,
            &call_id,
            cseq,
            &self.settings,
        );

        let resp = self.send(&msg).await?;
        let status = utils::parse_status_code(&resp)?;

        if (status == 401 || status == 407) && self.auth_method == crate::sip::AuthMethod::Md5 {
            let (realm, nonce) = utils::extract_auth_params(&resp)
                .context("Cannot extract WWW-Authenticate params")?;

            let auth_msg = build_register_with_auth(
                &self.username,
                &self.password,
                &self.domain,
                &local,
                &self.local_tag,
                &self.new_branch(),
                &self.new_call_id(),
                self.next_cseq().await,
                &realm,
                &nonce,
                &self.settings,
            );

            let resp2 = self.send(&auth_msg).await?;
            let status2 = utils::parse_status_code(&resp2)?;

            if status2 == 200 {
                log::info!("Registration successful (MD5 auth)");
                *self.registered.lock().await = true;
                return Ok(true);
            }
            log::error!("Auth registration failed (status={})", status2);
            *self.registered.lock().await = false;
            return Ok(false);
        }

        if status == 200 {
            log::info!("Registration successful");
            *self.registered.lock().await = true;
            return Ok(true);
        }

        log::error!("Registration failed (status={})", status);
        *self.registered.lock().await = false;
        Ok(false)
    }

    /// UNREGISTER with the server (REGISTER with Expires: 0). Returns true on success.
    pub async fn unregister(&self) -> Result<bool> {
        let local = self.local_addr_str();
        let branch = self.new_branch();
        let call_id = self.new_call_id();
        let cseq = self.next_cseq().await;

        let mut settings = self.settings.clone();
        settings.register_expiry = 0; // set expiry to 0 to unregister

        let (msg, _cid, _cs) = build_register(
            &self.username,
            &self.domain,
            &local,
            &self.local_tag,
            &branch,
            &call_id,
            cseq,
            &settings,
        );

        let resp = self.send(&msg).await?;
        let status = utils::parse_status_code(&resp)?;

        if (status == 401 || status == 407) && self.auth_method == crate::sip::AuthMethod::Md5 {
            let (realm, nonce) = utils::extract_auth_params(&resp)
                .context("Cannot extract WWW-Authenticate params")?;

            let auth_msg = build_register_with_auth(
                &self.username,
                &self.password,
                &self.domain,
                &local,
                &self.local_tag,
                &self.new_branch(),
                &self.new_call_id(),
                self.next_cseq().await,
                &realm,
                &nonce,
                &settings,
            );

            let resp2 = self.send(&auth_msg).await?;
            let status2 = utils::parse_status_code(&resp2)?;

            if status2 == 200 {
                log::info!("Unregistration successful (MD5 auth)");
                *self.registered.lock().await = false;
                return Ok(true);
            }
            log::error!("Auth unregistration failed (status={})", status2);
            return Ok(false);
        }

        if status == 200 {
            log::info!("Unregistration successful");
            *self.registered.lock().await = false;
            return Ok(true);
        }

        log::error!("Unregistration failed (status={})", status);
        Ok(false)
    }

    // ── Invite ────────────────────────────────────────────

    /// Send INVITE to establish a call. Returns true if call is set up.
    pub async fn invite(&mut self, target_uri: &str) -> Result<bool> {
        let call_id = self.new_call_id();
        let branch = self.new_branch();
        let cseq = self.next_cseq().await;
        let local = self.local_addr_str();
        let sdp_body = sdp::build_sdp_default(
            &self.username,
            &self.local_addr.ip().to_string(),
            self.rtp_port_start,
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
        );

        let resp = self.send(&msg).await?;
        let status = utils::parse_status_code(&resp)?;

        self.call_id = Some(call_id.clone());
        self.remote_tag = utils::extract_to_tag(&resp);

        if (100..200).contains(&status) {
            log::info!("Got provisional response {} — waiting for final...", status);
            let final_resp = self.recv_extra(30000).await?;
            let final_status = utils::parse_status_code(&final_resp)?;
            self.remote_tag = utils::extract_to_tag(&final_resp);

            if final_status == 200 {
                self.send_ack(target_uri, &local, &call_id, cseq).await?;
                self.in_call = true;
                self.remote_rtp_addr = crate::service::watcher::parse_sdp_connection(&final_resp);
                log::info!("Call established! Remote RTP: {:?}", self.remote_rtp_addr);
                return Ok(true);
            }
            log::error!("Call failed (final status={})", final_status);
            return Ok(false);
        }

        if status == 200 {
            self.send_ack(target_uri, &local, &call_id, cseq).await?;
            self.in_call = true;
            self.remote_rtp_addr = crate::service::watcher::parse_sdp_connection(&resp);
            log::info!("Call established! Remote RTP: {:?}", self.remote_rtp_addr);
            return Ok(true);
        }

        log::error!("Call failed (status={})", status);
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
        );
        self.transport
            .send_to(ack.as_bytes(), self.server_addr)
            .await?;
        Ok(())
    }

    // ── Bye ───────────────────────────────────────────────

    /// Send BYE to end the active call.
    pub async fn bye(&mut self) -> Result<bool> {
        if !self.in_call {
            log::warn!("No active call");
            return Ok(false);
        }

        let call_id = self.call_id.as_ref().context("No call_id")?;
        let remote_tag = self.remote_tag.as_ref().context("No remote_tag")?;
        let local = self.local_addr_str();

        let msg = build_bye(
            &self.username,
            &self.domain,
            &local,
            &self.local_tag,
            remote_tag,
            call_id,
            self.next_cseq().await,
            &self.new_branch(),
            &self.settings,
        );

        let resp = self.send(&msg).await?;
        let status = utils::parse_status_code(&resp)?;

        if status == 200 {
            log::info!("Call ended successfully");
            self.in_call = false;
            self.call_id = None;
            self.remote_tag = None;
            self.remote_rtp_addr = None;
            self.rtp_receiver = None;
            return Ok(true);
        }

        log::error!("Failed to end call (status={})", status);
        Ok(false)
    }

    // ── Cancel ────────────────────────────────────────────

    /// Send CANCEL for the current INVITE transaction.
    pub async fn cancel(&self) -> Result<bool> {
        let call_id = self.call_id.as_ref().context("No active call")?;
        let local = self.local_addr_str();

        let msg = build_cancel(
            &self.username,
            &self.domain,
            &local,
            &self.local_tag,
            call_id,
            self.next_cseq().await,
            &self.new_branch(),
            &self.settings,
        );

        let resp = self.send(&msg).await?;
        let status = utils::parse_status_code(&resp)?;
        log::info!("Cancel response: {}", status);
        Ok(status == 200 || status == 487)
    }
}
