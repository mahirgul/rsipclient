//! Transport layer for SIP client — UDP and TLS

use anyhow::{Context, Result};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio_native_tls::{native_tls::TlsConnector as NativeTlsConnector, TlsConnector, TlsStream};

// ── UDP Transport ──────────────────────────────────────

pub struct UdpTransport {
    socket: UdpSocket,
}

impl UdpTransport {
    pub async fn new(bind_addr: SocketAddr) -> Result<Self> {
        let socket = UdpSocket::bind(bind_addr)
            .await
            .context("Failed to bind UDP socket")?;
        log::info!("UDP socket bound to {}", socket.local_addr()?);
        Ok(Self { socket })
    }

    pub async fn send_to(&self, data: &[u8], target: SocketAddr) -> Result<usize> {
        let n = self
            .socket
            .send_to(data, target)
            .await
            .context("Failed to send UDP packet")?;
        Ok(n)
    }

    pub async fn recv_timeout(&self, timeout_ms: u64) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; 65535];
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            self.socket.recv_from(&mut buf),
        )
        .await
        .context("Receive timed out")?
        .context("Failed to receive UDP packet")?;

        let (n, _src) = result;
        buf.truncate(n);
        Ok(buf)
    }

    /// Try to receive a packet with a short timeout (non-blocking).
    /// Returns None if nothing arrived within `timeout_ms`.
    pub async fn try_recv(&self, timeout_ms: u64) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; 65535];
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            self.socket.recv_from(&mut buf),
        )
        .await;

        match result {
            Ok(Ok((n, _src))) => {
                buf.truncate(n);
                Some(buf)
            }
            _ => None,
        }
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.socket.local_addr().map_err(Into::into)
    }
}

// ── TCP Transport ──────────────────────────────────────

/// Plain TCP transport for SIP.
pub struct TcpTransport {
    stream: tokio::sync::Mutex<TcpStream>,
    local_addr: SocketAddr,
    /// Buffer for partial reads
    read_buf: tokio::sync::Mutex<Vec<u8>>,
}

impl TcpTransport {
    /// Create a new TCP transport by connecting to `server_addr`.
    pub async fn new(_bind_addr: SocketAddr, server_addr: SocketAddr) -> Result<Self> {
        let tcp = TcpStream::connect(server_addr)
            .await
            .context(format!("Failed to connect TCP to {}", server_addr))?;

        let local_addr = tcp.local_addr()?;

        log::info!(
            "TCP connection established to {} (local: {})",
            server_addr,
            local_addr
        );

        Ok(Self {
            stream: tokio::sync::Mutex::new(tcp),
            local_addr,
            read_buf: tokio::sync::Mutex::new(Vec::new()),
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.local_addr)
    }

    /// Send a complete SIP message over TCP.
    pub async fn send_to(&self, data: &[u8], _target: SocketAddr) -> Result<usize> {
        let mut stream = self.stream.lock().await;
        stream
            .write_all(data)
            .await
            .context("Failed to send TCP data")?;
        stream.flush().await.context("Failed to flush TCP stream")?;
        Ok(data.len())
    }

    /// Receive a single SIP message with timeout.
    pub async fn recv_timeout(&self, timeout_ms: u64) -> Result<Vec<u8>> {
        tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            self.recv_sip_message(),
        )
        .await
        .context("TCP receive timed out")?
    }

    /// Try to receive with a short timeout. Returns None if nothing arrived.
    pub async fn try_recv(&self, timeout_ms: u64) -> Option<Vec<u8>> {
        match tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            self.recv_sip_message(),
        )
        .await
        {
            Ok(Ok(msg)) => Some(msg),
            _ => None,
        }
    }

    /// Internal: read from TCP stream until we have a complete SIP message.
    async fn recv_sip_message(&self) -> Result<Vec<u8>> {
        let mut buf = [0u8; 8192];

        loop {
            // Check if we already have a complete message
            {
                let mut read_buf = self.read_buf.lock().await;
                if let Some(msg) = extract_sip_message(&mut read_buf) {
                    return Ok(msg);
                }
            }

            // Read more data from stream
            let n = {
                let mut stream = self.stream.lock().await;
                stream
                    .read(&mut buf)
                    .await
                    .context("Failed to read from TCP stream")?
            };

            if n == 0 {
                // Connection closed
                let mut read_buf = self.read_buf.lock().await;
                if !read_buf.is_empty() {
                    return Ok(std::mem::take(&mut *read_buf));
                }
                anyhow::bail!("TCP connection closed by peer");
            }

            let mut read_buf = self.read_buf.lock().await;
            read_buf.extend_from_slice(&buf[..n]);

            // Safety limit
            if read_buf.len() > 65536 {
                anyhow::bail!("SIP message too large (>64 KiB)");
            }
        }
    }
}

// ── TLS Transport ──────────────────────────────────────

/// TLS-wrapped TCP transport for SIP (SIPS).
/// The inner TLS stream is behind a Mutex so that `&self` methods work.
pub struct TlsTransport {
    stream: tokio::sync::Mutex<TlsStream<TcpStream>>,
    local_addr: SocketAddr,
    /// Buffer for partial reads
    read_buf: tokio::sync::Mutex<Vec<u8>>,
}

impl TlsTransport {
    /// Create a new TLS transport by connecting to `server_addr` and performing TLS handshake.
    pub async fn new(
        _bind_addr: SocketAddr,
        server_addr: SocketAddr,
        domain: &str,
    ) -> Result<Self> {
        let tcp = TcpStream::connect(server_addr)
            .await
            .context(format!("Failed to connect TCP to {}", server_addr))?;

        let local_addr = tcp.local_addr()?;

        // Build TLS connector (accept all certs — configurable later)
        let mut native_connector = NativeTlsConnector::builder();
        native_connector
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true);
        let connector: TlsConnector = native_connector.build()?.into();

        let stream = connector
            .connect(domain, tcp)
            .await
            .context("TLS handshake failed")?;

        log::info!(
            "TLS connection established to {} (local: {})",
            server_addr,
            local_addr
        );

        Ok(Self {
            stream: tokio::sync::Mutex::new(stream),
            local_addr,
            read_buf: tokio::sync::Mutex::new(Vec::new()),
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.local_addr)
    }

    /// Send a complete SIP message.
    pub async fn send_to(&self, data: &[u8], _target: SocketAddr) -> Result<usize> {
        let mut stream = self.stream.lock().await;
        stream
            .write_all(data)
            .await
            .context("Failed to send TLS data")?;
        stream.flush().await.context("Failed to flush TLS stream")?;
        Ok(data.len())
    }

    /// Receive a single SIP message with timeout.
    pub async fn recv_timeout(&self, timeout_ms: u64) -> Result<Vec<u8>> {
        tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            self.recv_sip_message(),
        )
        .await
        .context("TLS receive timed out")?
    }

    /// Try to receive with a short timeout. Returns None if nothing arrived.
    pub async fn try_recv(&self, timeout_ms: u64) -> Option<Vec<u8>> {
        match tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            self.recv_sip_message(),
        )
        .await
        {
            Ok(Ok(msg)) => Some(msg),
            _ => None,
        }
    }

    /// Internal: read from TLS stream until we have a complete SIP message.
    async fn recv_sip_message(&self) -> Result<Vec<u8>> {
        let mut buf = [0u8; 8192];

        loop {
            // Check if we already have a complete message
            {
                let mut read_buf = self.read_buf.lock().await;
                if let Some(msg) = extract_sip_message(&mut read_buf) {
                    return Ok(msg);
                }
            }

            // Read more data from stream
            let n = {
                let mut stream = self.stream.lock().await;
                stream
                    .read(&mut buf)
                    .await
                    .context("Failed to read from TLS stream")?
            };

            if n == 0 {
                // Connection closed
                let mut read_buf = self.read_buf.lock().await;
                if !read_buf.is_empty() {
                    return Ok(std::mem::take(&mut *read_buf));
                }
                anyhow::bail!("TLS connection closed by peer");
            }

            let mut read_buf = self.read_buf.lock().await;
            read_buf.extend_from_slice(&buf[..n]);

            // Safety limit
            if read_buf.len() > 65536 {
                anyhow::bail!("SIP message too large (>64 KiB)");
            }
        }
    }
}

// ── Shared stream parsing helpers ──────────────────────

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn parse_content_length(headers: &str) -> Option<usize> {
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
fn extract_sip_message(buf: &mut Vec<u8>) -> Option<Vec<u8>> {
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
}
