//! TLS Transport implementation for SIP

use anyhow::{Context, Result};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_native_tls::{
    native_tls::{self, TlsConnector as NativeTlsConnector},
    TlsConnector, TlsStream,
};

/// TLS connection verification configuration.
#[derive(Clone, Debug)]
pub struct TlsConfig {
    /// Verify the server certificate chain against system roots / custom CA.
    pub verify_cert: bool,
    /// Verify the server hostname matches the certificate.
    pub verify_hostname: bool,
    /// Optional path to a PEM file with custom CA certificates.
    pub ca_cert: Option<String>,
}

impl Default for TlsConfig {
    fn default() -> Self {
        TlsConfig {
            verify_cert: true,
            verify_hostname: true,
            ca_cert: None,
        }
    }
}

/// TLS-wrapped TCP transport for SIP (SIPS).
/// The inner TLS stream is behind a Mutex so that `&self` methods work.
pub struct TlsTransport {
    stream: tokio::sync::Mutex<TlsStream<TcpStream>>,
    local_addr: SocketAddr,
    /// Remote address this stream is connected to — see the identical field
    /// on `TcpTransport` for why TLS/TCP can report a fixed "source".
    peer_addr: SocketAddr,
    /// Buffer for partial reads
    read_buf: tokio::sync::Mutex<Vec<u8>>,
}

impl TlsTransport {
    /// Create a new TLS transport by connecting to `server_addr` and performing TLS handshake.
    pub async fn new(
        _bind_addr: SocketAddr,
        server_addr: SocketAddr,
        domain: &str,
        config: &TlsConfig,
    ) -> Result<Self> {
        let tcp = TcpStream::connect(server_addr)
            .await
            .context(format!("Failed to connect TCP to {}", server_addr))?;

        let local_addr = tcp.local_addr()?;

        // Build TLS connector with certificate/hostname verification per config.
        let mut native_connector = NativeTlsConnector::builder();
        native_connector
            .danger_accept_invalid_certs(!config.verify_cert)
            .danger_accept_invalid_hostnames(!config.verify_hostname);

        if let Some(ref ca_path) = config.ca_cert {
            let pem = std::fs::read(ca_path)
                .with_context(|| format!("Failed to read TLS CA certificate '{}'", ca_path))?;
            let cert = native_tls::Certificate::from_pem(&pem)
                .context("Invalid TLS CA certificate PEM")?;
            native_connector.add_root_certificate(cert);
        }

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
            peer_addr: server_addr,
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
    pub async fn recv_timeout(&self, timeout_ms: u64) -> Result<(Vec<u8>, SocketAddr)> {
        let msg = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            self.recv_sip_message(),
        )
        .await
        .context("TLS receive timed out")??;
        Ok((msg, self.peer_addr))
    }

    /// Try to receive with a short timeout. Returns None if nothing arrived.
    pub async fn try_recv(&self, timeout_ms: u64) -> Option<(Vec<u8>, SocketAddr)> {
        match tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            self.recv_sip_message(),
        )
        .await
        {
            Ok(Ok(msg)) => Some((msg, self.peer_addr)),
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
