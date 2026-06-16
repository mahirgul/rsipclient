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
        if !self.in_call {
            log::warn!("No active call to hold");
            return Ok(false);
        }
        let call_id = self.call_id.as_ref().context("No call_id")?;
        let remote_tag = self.remote_tag.as_ref().context("No remote_tag")?;
        let remote_uri = self.remote_uri.as_ref().context("No remote_uri")?;
        let local = self.local_addr_str();

        let msg = crate::sip::transfer::build_hold(
            &self.username,
            &self.domain,
            remote_uri,
            &self.local_addr.ip().to_string(),
            &local,
            &self.local_tag,
            remote_tag,
            call_id,
            self.next_cseq().await,
            &self.new_branch(),
            self.rtp_port_start,
            &self.settings,
            false, // resume = false
            &self.codec,
            self.transport.via_str(),
        );

        let resp = self.send(&msg).await?;
        let status = utils::parse_status_code(&resp)?;
        if status == 200 {
            self.held = true;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Resume the active call from hold
    /// Sends a re-INVITE containing a sendrecv audio stream in the SDP body.
    pub async fn resume(&mut self) -> Result<bool> {
        if !self.in_call {
            log::warn!("No active call to resume");
            return Ok(false);
        }
        let call_id = self.call_id.as_ref().context("No call_id")?;
        let remote_tag = self.remote_tag.as_ref().context("No remote_tag")?;
        let remote_uri = self.remote_uri.as_ref().context("No remote_uri")?;
        let local = self.local_addr_str();

        let msg = crate::sip::transfer::build_hold(
            &self.username,
            &self.domain,
            remote_uri,
            &self.local_addr.ip().to_string(),
            &local,
            &self.local_tag,
            remote_tag,
            call_id,
            self.next_cseq().await,
            &self.new_branch(),
            self.rtp_port_start,
            &self.settings,
            true, // resume = true
            &self.codec,
            self.transport.via_str(),
        );

        let resp = self.send(&msg).await?;
        let status = utils::parse_status_code(&resp)?;
        if status == 200 {
            self.held = false;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Transfer the active call to a target URI
    /// Sends a REFER request instructing the server/peer to connect to the target.
    pub async fn transfer(&mut self, target_uri: &str) -> Result<bool> {
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
        let status = utils::parse_status_code(&resp)?;
        Ok(status == 200 || status == 202)
    }
}
