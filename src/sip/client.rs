//! SIP Client -- struct definition and low-level helpers

use crate::sip::settings::SipSettings;
use crate::sip::transport::Transport;
use crate::sip::utils;
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Auth method for REGISTER
#[derive(Clone, Debug, PartialEq)]
pub enum AuthMethod {
    None,
    Md5,
}

/// Main SIP client state
#[allow(dead_code)]
pub struct SipClient {
    pub server_addr: SocketAddr,
    pub local_addr: SocketAddr,
    pub username: String,
    pub password: String,
    pub domain: String,
    pub local_tag: String,
    pub cseq: Arc<Mutex<u32>>,
    pub transport: Transport,
    pub rtp_port_start: u16,
    pub rtp_port_end: u16,
    pub auth_method: AuthMethod,
    pub settings: SipSettings,
    pub codec: String,
    pub(crate) call_id: Option<String>,
    /// CSeq used for the outstanding INVITE (needed for CANCEL to match RFC 3261)
    pub(crate) invite_cseq: Option<u32>,
    pub remote_tag: Option<String>,
    pub in_call: bool,
    pub(crate) call_start_time: Option<std::time::Instant>,
    pub held: bool,
    pub registered: Arc<Mutex<bool>>,
    pub remote_rtp_addr: Option<SocketAddr>,
    pub remote_uri: Option<String>,
    pub rtp_receiver: Option<crate::rtp::receiver::RtpReceiver>,
    /// Actual local RTP port bound by the receiver (not the range start).
    pub rtp_port: Option<u16>,
    /// Transaction layer manager for RFC 3261 §17 transaction FSM and message demux
    pub transaction_mgr: Arc<crate::sip::transaction::TransactionManager>,
    /// Negotiated RFC 4028 session-timer interval (seconds) for the active
    /// dialog, taken from the peer's Session-Expires header. `None` when
    /// session timers aren't in use or nothing was negotiated yet.
    pub session_expires_secs: Option<u32>,
}

impl SipClient {
    pub async fn new(
        transport: Transport,
        server_addr: SocketAddr,
        local_addr: SocketAddr,
        username: String,
        password: String,
        domain: String,
        rtp_port_start: u16,
        rtp_port_end: u16,
        auth_method: AuthMethod,
        settings: SipSettings,
        codec: String,
    ) -> Result<Self> {
        let client = Self {
            server_addr,
            local_addr,
            username,
            password,
            domain,
            local_tag: utils::short_id("tag-"),
            cseq: Arc::new(Mutex::new(1)),
            transport,
            rtp_port_start,
            rtp_port_end,
            auth_method,
            settings,
            codec,
            call_id: None,
            invite_cseq: None,
            remote_tag: None,
            in_call: false,
            call_start_time: None,
            held: false,
            registered: Arc::new(Mutex::new(false)),
            remote_rtp_addr: None,
            remote_uri: None,
            rtp_receiver: None,
            rtp_port: None,
            transaction_mgr: Arc::new(crate::sip::transaction::TransactionManager::new()),
            session_expires_secs: None,
        };
        client.transport.set_peer_filter(server_addr);
        Ok(client)
    }

    pub(crate) async fn next_cseq(&self) -> u32 {
        let mut c = self.cseq.lock().await;
        let val = *c;
        *c += 1;
        val
    }

    pub(crate) fn new_call_id(&self) -> String {
        format!("{}@{}", Uuid::new_v4(), self.domain)
    }

    pub(crate) fn new_branch(&self) -> String {
        format!("z9hG4bK-{}", Uuid::new_v4())
    }

    pub(crate) fn local_addr_str(&self) -> String {
        format!("{}:{}", self.local_addr.ip(), self.local_addr.port())
    }

    /// Send NAT keep-alive packet (double CRLF)
    pub async fn send_keepalive(&self) -> Result<()> {
        log::debug!("Sending NAT keep-alive (double CRLF)...");
        self.transport
            .send_to(b"\r\n\r\n", self.server_addr)
            .await?;
        Ok(())
    }

    pub(crate) async fn send(&self, msg: &str) -> Result<String> {
        log::debug!("--- SEND ---\n{}", msg);
        let first_line = msg.lines().next().unwrap_or("");
        let method = first_line
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();

        if method == "INVITE" {
            let resp = self
                .transaction_mgr
                .execute_invite(
                    &self.transport,
                    self.server_addr,
                    msg,
                    self.cseq.clone(),
                    |status, r| {
                        log::info!("Provisional response {} received for INVITE", status);
                        log::debug!("--- RECV PROVISIONAL ---\n{}", r);
                    },
                )
                .await?;
            log::debug!("--- RECV ---\n{}", resp);
            Ok(resp)
        } else {
            let resp = self
                .transaction_mgr
                .execute_non_invite(&self.transport, self.server_addr, msg)
                .await?;
            log::debug!("--- RECV ---\n{}", resp);
            Ok(resp)
        }
    }

    pub(crate) async fn recv_extra(&self, timeout_ms: u64) -> Result<String> {
        let (buf, _src) = self
            .transport
            .recv_timeout(timeout_ms)
            .await
            .context("Timeout waiting for response")?;
        let resp = String::from_utf8_lossy(&buf).to_string();
        log::debug!("--- RECV ---\n{}", resp);
        crate::service::logger::record_sip_trace(
            "IN",
            &self.username,
            &resp,
            self.transport.via_str(),
        );
        Ok(resp)
    }

    /// Try to receive an unsolicited message (for incoming call detection).
    /// Returns None if nothing received within `timeout_ms`.
    pub async fn try_recv(&self, timeout_ms: u64) -> Option<String> {
        // 1. Check if an incoming request was already queued
        if let Some(req) = self.transaction_mgr.try_pop_incoming_request() {
            crate::service::logger::record_sip_trace(
                "IN",
                &self.username,
                &req.raw,
                self.transport.via_str(),
            );
            return Some(req.raw);
        }

        // 2. Poll transport for incoming packet. `src` is the packet's real
        // sender (not `self.server_addr`), so process_incoming can match/
        // reply to peers other than the configured server (e.g. an SBC that
        // answers from a different IP) and any auto-reply it sends goes back
        // to whoever actually sent the request.
        let (buf, src) = self.transport.try_recv(timeout_ms).await?;
        self.transaction_mgr
            .process_incoming(&self.transport, &buf, src)
            .await;

        // 3. Return incoming request if one was queued. process_incoming
        // fully disposes of every request it sees — either by auto-replying
        // (retransmit cache, OPTIONS, PRACK) or by queuing it here — so
        // there is nothing left to fall back to when this comes back empty;
        // re-parsing the raw buffer would re-deliver a request that was
        // already handled and replied to above.
        if let Some(req) = self.transaction_mgr.try_pop_incoming_request() {
            crate::service::logger::record_sip_trace(
                "IN",
                &self.username,
                &req.raw,
                self.transport.via_str(),
            );
            return Some(req.raw);
        }

        None
    }

    /// Send a SIP MESSAGE text chat request (RFC 3428)
    pub async fn send_message(&self, target_uri: &str, text_body: &str) -> Result<String> {
        crate::sip::utils::validate_header_value(target_uri, "message target")?;
        let branch = self.new_branch();
        let call_id = self.new_call_id();
        let cseq = self.next_cseq().await;
        let msg = crate::sip::messages::build_message(
            target_uri,
            &self.username,
            &self.domain,
            &self.local_addr_str(),
            &self.local_tag,
            &branch,
            &call_id,
            cseq,
            text_body,
            &self.settings,
            self.transport.via_str(),
        );
        self.send(&msg).await
    }

    /// Send out-of-band SIP INFO DTMF digit (RFC 6086)
    pub async fn send_info_dtmf(&self, digit: char, duration_ms: u32) -> Result<String> {
        crate::sip::utils::validate_dtmf_digit(digit)?;
        let remote_uri = self
            .remote_uri
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Not in an active call"))?;
        let remote_tag = self
            .remote_tag
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("No remote tag"))?;
        let call_id = self
            .call_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("No active Call-ID"))?;
        let branch = self.new_branch();
        let cseq = self.next_cseq().await;

        let msg = crate::sip::messages::build_info_dtmf(
            remote_uri,
            &self.username,
            &self.domain,
            &self.local_addr_str(),
            &self.local_tag,
            remote_tag,
            call_id,
            cseq,
            &branch,
            digit,
            duration_ms,
            &self.settings,
            self.transport.via_str(),
        );
        self.send(&msg).await
    }

    /// Send a SIP SUBSCRIBE request (RFC 6665 / RFC 3265)
    #[allow(dead_code)]
    pub async fn send_subscribe(
        &self,
        target_uri: &str,
        event_type: &str,
        expires_secs: u32,
    ) -> Result<String> {
        crate::sip::utils::validate_header_value(target_uri, "subscribe target")?;
        let branch = self.new_branch();
        let call_id = self.new_call_id();
        let cseq = self.next_cseq().await;
        let msg = crate::sip::messages::build_subscribe(
            target_uri,
            &self.username,
            &self.domain,
            &self.local_addr_str(),
            &self.local_tag,
            &branch,
            &call_id,
            cseq,
            event_type,
            expires_secs,
            &self.settings,
            self.transport.via_str(),
        );
        self.send(&msg).await
    }

    /// Send a PRACK request for reliable provisional responses (RFC 3262)
    #[allow(dead_code)]
    pub async fn send_prack(&self, rseq: u32, invite_cseq: u32) -> Result<String> {
        let remote_uri = self
            .remote_uri
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Not in an active call"))?;
        let remote_tag = self
            .remote_tag
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("No remote tag"))?;
        let call_id = self
            .call_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("No active Call-ID"))?;
        let branch = self.new_branch();
        let cseq = self.next_cseq().await;

        let msg = crate::sip::messages::build_prack(
            remote_uri,
            &self.username,
            &self.domain,
            &self.local_addr_str(),
            &self.local_tag,
            remote_tag,
            call_id,
            cseq,
            rseq,
            invite_cseq,
            &branch,
            &self.settings,
            self.transport.via_str(),
        );
        self.send(&msg).await
    }
}
