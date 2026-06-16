//! UDP Transport implementation for SIP

use anyhow::{Context, Result};
use std::net::SocketAddr;
use tokio::net::UdpSocket;

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
        loop {
            let result = tokio::time::timeout(
                std::time::Duration::from_millis(timeout_ms),
                self.socket.recv_from(&mut buf),
            )
            .await;

            let result = match result {
                Ok(res) => res,
                Err(_) => anyhow::bail!("Receive timed out"),
            };

            let (n, _src) = match result {
                Ok(val) => val,
                Err(ref e) if e.kind() == std::io::ErrorKind::ConnectionReset => {
                    log::debug!("Ignoring Windows UDP ConnectionReset error (WSAECONNRESET)");
                    continue;
                }
                Err(e) => anyhow::bail!("Failed to receive UDP packet: {}", e),
            };

            let is_crlf_only = buf[..n].iter().all(|&b| b == b'\r' || b == b'\n');
            if is_crlf_only {
                log::debug!("Received UDP keep-alive/CRLF packet, ignoring.");
                continue;
            }
            buf.truncate(n);
            return Ok(buf);
        }
    }

    /// Try to receive a packet with a short timeout (non-blocking).
    /// Returns None if nothing arrived within `timeout_ms`.
    pub async fn try_recv(&self, timeout_ms: u64) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; 65535];
        loop {
            let result = tokio::time::timeout(
                std::time::Duration::from_millis(timeout_ms),
                self.socket.recv_from(&mut buf),
            )
            .await;

            match result {
                Ok(Ok((n, _src))) => {
                    let is_crlf_only = buf[..n].iter().all(|&b| b == b'\r' || b == b'\n');
                    if is_crlf_only {
                        log::debug!("Received UDP keep-alive/CRLF packet in try_recv, ignoring.");
                        continue;
                    }
                    buf.truncate(n);
                    return Some(buf);
                }
                Ok(Err(ref e)) if e.kind() == std::io::ErrorKind::ConnectionReset => {
                    log::debug!("Ignoring Windows UDP ConnectionReset error in try_recv");
                    continue;
                }
                _ => return None,
            }
        }
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.socket.local_addr().map_err(Into::into)
    }
}
