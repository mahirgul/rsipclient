//! TCP Transport implementation for SIP

use anyhow::{Context, Result};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

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
                if let Some(msg) = super::extract_sip_message(&mut read_buf) {
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
