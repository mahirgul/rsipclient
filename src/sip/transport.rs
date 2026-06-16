//! Transport layer for SIP client — UDP, TCP, and TLS

pub mod tcp;
pub mod tls;
pub mod udp;

pub use tcp::TcpTransport;
pub use tls::TlsTransport;
pub use udp::UdpTransport;

use anyhow::Result;
use std::net::SocketAddr;

// ── Shared stream parsing helpers ──────────────────────

pub(crate) fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

pub(crate) fn parse_content_length(headers: &str) -> Option<usize> {
    for line in headers.lines() {
        let lower = line.to_lowercase();
        let trimmed = lower.trim();
        if let Some(val) = trimmed.strip_prefix("content-length:") {
            return val.trim().parse().ok();
        }
        if let Some(val) = trimmed.strip_prefix("l:") {
            return val.trim().parse().ok();
        }
    }
    None
}

/// Try to extract a complete SIP message from the buffer.
pub(crate) fn extract_sip_message(buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    // Discard any leading CRLF/LF/CR keep-alive bytes
    let non_crlf_pos = buf.iter().position(|&b| b != b'\r' && b != b'\n');
    match non_crlf_pos {
        Some(0) => {}
        Some(pos) => {
            buf.drain(..pos);
        }
        None => {
            // Buffer contains only CRLF bytes, clear it
            buf.clear();
            return None;
        }
    }

    let header_end = find_subsequence(buf, b"\r\n\r\n")? + 4;

    let headers = std::str::from_utf8(&buf[..header_end]).ok()?;
    let content_length = parse_content_length(headers).unwrap_or(0);

    let total_len = header_end + content_length;

    if buf.len() >= total_len {
        let msg = buf[..total_len].to_vec();
        buf.drain(..total_len);
        Some(msg)
    } else {
        None
    }
}

// ── Transport Enum ─────────────────────────────────────

/// Unified transport — UDP, TCP, or TLS, all with `&self` API.
pub enum Transport {
    Udp(UdpTransport),
    Tcp(TcpTransport),
    Tls(Box<TlsTransport>),
}

impl Transport {
    /// Create a UDP transport.
    pub async fn new_udp(bind_addr: SocketAddr) -> Result<Self> {
        Ok(Transport::Udp(UdpTransport::new(bind_addr).await?))
    }

    /// Create a TCP transport.
    pub async fn new_tcp(bind_addr: SocketAddr, server_addr: SocketAddr) -> Result<Self> {
        Ok(Transport::Tcp(
            TcpTransport::new(bind_addr, server_addr).await?,
        ))
    }

    /// Create a TLS transport.
    pub async fn new_tls(
        bind_addr: SocketAddr,
        server_addr: SocketAddr,
        domain: &str,
    ) -> Result<Self> {
        Ok(Transport::Tls(Box::new(
            TlsTransport::new(bind_addr, server_addr, domain).await?,
        )))
    }

    pub async fn send_to(&self, data: &[u8], target: SocketAddr) -> Result<usize> {
        match self {
            Transport::Udp(udp) => udp.send_to(data, target).await,
            Transport::Tcp(tcp) => tcp.send_to(data, target).await,
            Transport::Tls(tls) => tls.send_to(data, target).await,
        }
    }

    pub async fn recv_timeout(&self, timeout_ms: u64) -> Result<Vec<u8>> {
        match self {
            Transport::Udp(udp) => udp.recv_timeout(timeout_ms).await,
            Transport::Tcp(tcp) => tcp.recv_timeout(timeout_ms).await,
            Transport::Tls(tls) => tls.recv_timeout(timeout_ms).await,
        }
    }

    pub async fn try_recv(&self, timeout_ms: u64) -> Option<Vec<u8>> {
        match self {
            Transport::Udp(udp) => udp.try_recv(timeout_ms).await,
            Transport::Tcp(tcp) => tcp.try_recv(timeout_ms).await,
            Transport::Tls(tls) => tls.try_recv(timeout_ms).await,
        }
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        match self {
            Transport::Udp(udp) => udp.local_addr(),
            Transport::Tcp(tcp) => tcp.local_addr(),
            Transport::Tls(tls) => tls.local_addr(),
        }
    }

    pub fn via_str(&self) -> &'static str {
        match self {
            Transport::Udp(_) => "UDP",
            Transport::Tcp(_) => "TCP",
            Transport::Tls(_) => "TLS",
        }
    }
}
