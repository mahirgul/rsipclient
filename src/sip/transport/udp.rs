//! UDP Transport implementation for SIP

use anyhow::{Context, Result};
use std::collections::VecDeque;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, RwLock};
use tokio::net::UdpSocket;
use tokio::time::Instant;

/// Maximum number of source IPs the allow-list remembers beyond the
/// originally configured server address, so a long-lived client can't
/// accumulate unbounded memory from repeated learning.
const MAX_LEARNED_PEERS: usize = 4;

/// Bounded set of source IPs the client currently trusts for inbound SIP
/// traffic (SIP UDP spoofing protection).
///
/// `primary` is the configured server address and is always trusted.
/// `learned` holds additional IPs picked up when a *response* matched a
/// transaction we initiated (see `TransactionManager::process_incoming`) —
/// proof of legitimacy, since matching requires knowing our unguessable
/// branch value, unlike a blind/off-path spoofed inbound request. This lets
/// load-balanced/anycast/multi-homed deployments that answer from a
/// different IP keep working without disabling the filter outright.
struct PeerAllowList {
    primary: Option<IpAddr>,
    learned: VecDeque<IpAddr>,
}

impl PeerAllowList {
    fn is_allowed(&self, ip: IpAddr) -> bool {
        match self.primary {
            None => true, // No filter configured yet.
            Some(p) => p == ip || self.learned.contains(&ip),
        }
    }

    fn learn(&mut self, ip: IpAddr) {
        if self.primary == Some(ip) || self.learned.contains(&ip) {
            return;
        }
        self.learned.push_back(ip);
        if self.learned.len() > MAX_LEARNED_PEERS {
            self.learned.pop_front();
        }
    }
}

pub struct UdpTransport {
    socket: UdpSocket,
    allowed: Arc<RwLock<PeerAllowList>>,
}

impl UdpTransport {
    pub async fn new(bind_addr: SocketAddr) -> Result<Self> {
        let socket = UdpSocket::bind(bind_addr)
            .await
            .context("Failed to bind UDP socket")?;
        log::info!("UDP socket bound to {}", socket.local_addr()?);
        Ok(Self {
            socket,
            allowed: Arc::new(RwLock::new(PeerAllowList {
                primary: None,
                learned: VecDeque::new(),
            })),
        })
    }

    /// Set the primary trusted peer address (SIP spoofing protection).
    /// Resets any previously learned secondary addresses.
    pub fn set_peer_filter(&self, peer: SocketAddr) {
        if let Ok(mut lock) = self.allowed.write() {
            lock.primary = Some(peer.ip());
            lock.learned.clear();
        }
    }

    /// Learn an additional trusted source IP after a response from it was
    /// matched to a transaction we initiated.
    pub fn learn_peer(&self, peer: SocketAddr) {
        if let Ok(mut lock) = self.allowed.write() {
            lock.learn(peer.ip());
        }
    }

    pub async fn send_to(&self, data: &[u8], target: SocketAddr) -> Result<usize> {
        let n = self
            .socket
            .send_to(data, target)
            .await
            .context("Failed to send UDP packet")?;
        Ok(n)
    }

    pub async fn recv_timeout(&self, timeout_ms: u64) -> Result<(Vec<u8>, SocketAddr)> {
        let mut buf = vec![0u8; 65535];
        // A single fixed deadline for the whole call: packets filtered out
        // below (untrusted source, CRLF keep-alive, transient recv error)
        // must not extend how long the caller ends up waiting.
        let deadline = Instant::now() + std::time::Duration::from_millis(timeout_ms);
        loop {
            let result = tokio::time::timeout_at(deadline, self.socket.recv_from(&mut buf)).await;

            let result = match result {
                Ok(res) => res,
                Err(_) => anyhow::bail!("Receive timed out"),
            };

            let (n, src) = match result {
                Ok(val) => val,
                Err(ref e) if e.kind() == std::io::ErrorKind::ConnectionReset => {
                    log::debug!("Ignoring Windows UDP ConnectionReset error (WSAECONNRESET)");
                    continue;
                }
                Err(e) => anyhow::bail!("Failed to receive UDP packet: {}", e),
            };

            // Source-IP filtering is applied by the transaction layer, not
            // here: a *response* matching a transaction we initiated proves
            // its own legitimacy (via the unguessable branch) regardless of
            // source IP, so it must reach `process_incoming` to be checked —
            // dropping it at this layer would make a legitimate peer that
            // answers from a different IP (SBC, anycast) permanently
            // unreachable. See `TransactionManager::process_incoming`.
            let is_crlf_only = buf[..n].iter().all(|&b| b == b'\r' || b == b'\n');
            if is_crlf_only {
                log::debug!("Received UDP keep-alive/CRLF packet, ignoring.");
                continue;
            }
            buf.truncate(n);
            return Ok((buf, src));
        }
    }

    /// Try to receive a packet with a short timeout (non-blocking).
    /// Returns None if nothing arrived within `timeout_ms`.
    pub async fn try_recv(&self, timeout_ms: u64) -> Option<(Vec<u8>, SocketAddr)> {
        let mut buf = vec![0u8; 65535];
        let deadline = Instant::now() + std::time::Duration::from_millis(timeout_ms);
        loop {
            let result = tokio::time::timeout_at(deadline, self.socket.recv_from(&mut buf)).await;

            match result {
                Ok(Ok((n, src))) => {
                    // See the comment in `recv_timeout`: source-IP filtering
                    // happens in the transaction layer, not here.
                    let is_crlf_only = buf[..n].iter().all(|&b| b == b'\r' || b == b'\n');
                    if is_crlf_only {
                        log::debug!("Received UDP keep-alive/CRLF packet in try_recv, ignoring.");
                        continue;
                    }
                    buf.truncate(n);
                    return Some((buf, src));
                }
                Ok(Err(ref e)) if e.kind() == std::io::ErrorKind::ConnectionReset => {
                    log::debug!("Ignoring Windows UDP ConnectionReset error in try_recv");
                    continue;
                }
                _ => return None,
            }
        }
    }

    /// Whether `src` is currently a trusted peer (SIP spoofing protection).
    /// Checked by the transaction layer against *inbound requests* only —
    /// see `TransactionManager::process_incoming`.
    pub fn is_allowed(&self, src: SocketAddr) -> bool {
        self.allowed
            .read()
            .map(|g| g.is_allowed(src.ip()))
            .unwrap_or(true)
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.socket.local_addr().map_err(Into::into)
    }
}
