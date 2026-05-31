//! SIP Client -- struct definition and low-level helpers

use crate::sip::settings::SipSettings;
use crate::sip::transport::UdpTransport;
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
    pub transport: UdpTransport,
    pub rtp_port_start: u16,
    pub rtp_port_end: u16,
    pub auth_method: AuthMethod,
    pub settings: SipSettings,
    pub(crate) call_id: Option<String>,
    /// CSeq used for the outstanding INVITE (needed for CANCEL to match RFC 3261)
    pub(crate) invite_cseq: Option<u32>,
    pub remote_tag: Option<String>,
    pub in_call: bool,
    pub held: bool,
    pub registered: Arc<Mutex<bool>>,
    pub remote_rtp_addr: Option<SocketAddr>,
    pub remote_uri: Option<String>,
    pub rtp_receiver: Option<crate::rtp::receiver::RtpReceiver>,
}

impl SipClient {
    pub async fn new(
        server_addr: SocketAddr,
        mut local_addr: SocketAddr,
        username: String,
        password: String,
        domain: String,
        rtp_port_start: u16,
        rtp_port_end: u16,
        auth_method: AuthMethod,
        settings: SipSettings,
    ) -> Result<Self> {
        let transport = UdpTransport::new(local_addr).await?;
        local_addr = transport.local_addr()?;

        Ok(Self {
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
            call_id: None,
            invite_cseq: None,
            remote_tag: None,
            in_call: false,
            held: false,
            registered: Arc::new(Mutex::new(false)),
            remote_rtp_addr: None,
            remote_uri: None,
            rtp_receiver: None,
        })
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

    pub(crate) async fn send(&self, msg: &str) -> Result<String> {
        log::debug!("--- SEND ---\n{}", msg);
        self.transport
            .send_to(msg.as_bytes(), self.server_addr)
            .await?;

        let buf = self
            .transport
            .recv_timeout(5000)
            .await
            .context("Timeout waiting for response")?;

        let resp = String::from_utf8_lossy(&buf).to_string();
        log::debug!("--- RECV ---\n{}", resp);
        Ok(resp)
    }

    pub(crate) async fn recv_extra(&self, timeout_ms: u64) -> Result<String> {
        let buf = self
            .transport
            .recv_timeout(timeout_ms)
            .await
            .context("Timeout waiting for response")?;
        let resp = String::from_utf8_lossy(&buf).to_string();
        log::debug!("--- RECV ---\n{}", resp);
        Ok(resp)
    }

    /// Try to receive an unsolicited message (for incoming call detection).
    /// Returns None if nothing received within `timeout_ms`.
    pub async fn try_recv(&self, timeout_ms: u64) -> Option<String> {
        let buf = self.transport.try_recv(timeout_ms).await?;
        Some(String::from_utf8_lossy(&buf).to_string())
    }
}
