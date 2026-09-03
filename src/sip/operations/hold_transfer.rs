//! SIP call control operations (hold, resume, transfer).
//!
//! This file implements call hold (re-INVITE with sendonly/inactive SDP),
//! call resume (re-INVITE with sendrecv SDP), and call transfer (REFER request).

use crate::sip::client::SipClient;
use crate::sip::utils;
use anyhow::{Context, Result};

impl SipClient {
    /// Put the active call on hold
    /// Sends a re-INVITE containing a sendonly/inactive audio stream in the SDP body.
    pub async fn hold(&mut self) -> Result<bool> {
        self.hold_resume(false).await
    }

    /// Resume the active call from hold
    /// Sends a re-INVITE containing a sendrecv audio stream in the SDP body.
    pub async fn resume(&mut self) -> Result<bool> {
        self.hold_resume(true).await
    }

    /// Refresh the session timer for the active dialog (RFC 4028).
    ///
    /// Sends an in-dialog re-INVITE with the active SDP and Session-Expires header,
    /// retrying with Digest authentication if challenged.
    pub async fn refresh_session(&mut self) -> Result<bool> {
        if !self.in_call {
            log::warn!("No active call to refresh");
            return Ok(false);
        }
        log::info!("Refreshing active SIP session (RFC 4028)...");
        self.hold_resume(true).await
    }

    /// Shared hold/resume implementation.
    ///
    /// Sends a re-INVITE using the *actually bound* RTP port (not the range
    /// start), and retries with Digest auth if the server challenges with
    /// 401/407.
    async fn hold_resume(&mut self, resume: bool) -> Result<bool> {
        if !self.in_call {
            log::warn!("No active call to hold/resume");
            return Ok(false);
        }
        let call_id = self.call_id.clone().context("No call_id")?;
        let remote_tag = self.remote_tag.clone().context("No remote_tag")?;
        let remote_uri = self.remote_uri.clone().context("No remote_uri")?;
        let local = self.local_addr_str();
        let local_ip = self.local_addr.ip().to_string();
        let rtp_port = self.rtp_port.unwrap_or(self.rtp_port_start);

        let mut cseq = self.next_cseq().await;
        let msg = crate::sip::transfer::build_hold(
            &self.username,
            &self.domain,
            &remote_uri,
            &local_ip,
            &local,
            &self.local_tag,
            &remote_tag,
            &call_id,
            cseq,
            &self.new_branch(),
            rtp_port,
            &self.settings,
            resume,
            &self.codec,
            self.transport.via_str(),
        );

        let resp = self.send(&msg).await?;
        let mut status = utils::parse_status_code(&resp)?;

        // Handle 401/407 auth challenge on the re-INVITE.
        if (status == 401 || status == 407) && self.auth_method == crate::sip::AuthMethod::Md5 {
            let challenge = utils::extract_auth_challenge(&resp)
                .context("Cannot extract WWW-Authenticate params for re-INVITE")?;
            cseq = self.next_cseq().await;
            let auth_msg = crate::sip::transfer::build_hold_with_auth(
                &self.username,
                &self.password,
                &self.domain,
                &remote_uri,
                &local_ip,
                &local,
                &self.local_tag,
                &remote_tag,
                &call_id,
                cseq,
                &self.new_branch(),
                rtp_port,
                &self.settings,
                resume,
                &self.codec,
                self.transport.via_str(),
                &challenge,
            );
            let mut resp2 = self.send(&auth_msg).await?;
            status = utils::parse_status_code(&resp2)?;

            // If the server's nonce was stale, retry with the fresh nonce
            if let Some(fresh) =
                crate::sip::operations::register::stale_retry_challenge(status, &resp2)
            {
                log::info!("Hold/resume nonce was stale, retrying with the fresh one");
                cseq = self.next_cseq().await;
                let retry_msg = crate::sip::transfer::build_hold_with_auth(
                    &self.username,
                    &self.password,
                    &self.domain,
                    &remote_uri,
                    &local_ip,
                    &local,
                    &self.local_tag,
                    &remote_tag,
                    &call_id,
                    cseq,
                    &self.new_branch(),
                    rtp_port,
                    &self.settings,
                    resume,
                    &self.codec,
                    self.transport.via_str(),
                    &fresh,
                );
                resp2 = self.send(&retry_msg).await?;
                status = utils::parse_status_code(&resp2)?;
            }
        }

        if status == 200 {
            self.held = !resume;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Transfer the active call to a target URI
    /// Sends a REFER request instructing the server/peer to connect to the target.
    pub async fn transfer(&mut self, target_uri: &str) -> Result<bool> {
        crate::sip::utils::validate_header_value(target_uri, "transfer target")?;
        if !self.in_call {
            log::warn!("No active call to transfer");
            return Ok(false);
        }
        let call_id = self.call_id.as_ref().context("No call_id")?;
        let remote_tag = self.remote_tag.as_ref().context("No remote_tag")?;
        let remote_uri = self.remote_uri.as_ref().context("No remote_uri")?;
        let local = self.local_addr_str();

        let formatted_uri = if target_uri.starts_with("sip:") || target_uri.starts_with("sips:") {
            target_uri.to_string()
        } else if target_uri.contains('@') {
            format!("sip:{}", target_uri)
        } else {
            format!("sip:{}@{}", target_uri, self.domain)
        };
        let target_uri = &formatted_uri;

        let msg = crate::sip::transfer::build_refer(
            &self.username,
            &self.domain,
            remote_uri,
            target_uri,
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
        let mut status = utils::parse_status_code(&resp)?;

        // Handle 401/407 auth challenge on REFER (RFC 3515 / RFC 3261)
        if (status == 401 || status == 407) && self.auth_method == crate::sip::AuthMethod::Md5 {
            let challenge = utils::extract_auth_challenge(&resp)
                .context("Cannot extract WWW-Authenticate params for REFER")?;
            let mut auth_cseq = self.next_cseq().await;
            let auth_msg = crate::sip::transfer::build_refer_with_auth(
                &self.username,
                &self.password,
                &self.domain,
                remote_uri,
                target_uri,
                &local,
                &self.local_tag,
                remote_tag,
                call_id,
                auth_cseq,
                &self.new_branch(),
                &challenge,
                &self.settings,
                self.transport.via_str(),
            );
            let mut resp2 = self.send(&auth_msg).await?;
            status = utils::parse_status_code(&resp2)?;

            // If the REFER nonce was stale, retry with the fresh nonce
            if let Some(fresh) =
                crate::sip::operations::register::stale_retry_challenge(status, &resp2)
            {
                log::info!("Transfer REFER nonce was stale, retrying with the fresh one");
                auth_cseq = self.next_cseq().await;
                let retry_msg = crate::sip::transfer::build_refer_with_auth(
                    &self.username,
                    &self.password,
                    &self.domain,
                    remote_uri,
                    target_uri,
                    &local,
                    &self.local_tag,
                    remote_tag,
                    call_id,
                    auth_cseq,
                    &self.new_branch(),
                    &fresh,
                    &self.settings,
                    self.transport.via_str(),
                );
                resp2 = self.send(&retry_msg).await?;
                status = utils::parse_status_code(&resp2)?;
            }
        }

        Ok(status == 200 || status == 202)
    }
}
