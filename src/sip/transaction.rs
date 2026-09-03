//! SIP Transaction Layer (RFC 3261 §17)
//!
//! Provides:
//! - TransactionKey: matching by topmost Via `branch` parameter (`z9hG4bK-...`) and CSeq `method`.
//! - Client Non-INVITE Transaction FSM (Timer E / Timer F / Timer K) for REGISTER, BYE, INFO, REFER, MESSAGE, etc.
//! - Client INVITE Transaction FSM (Timer A / Timer B / Timer D) with provisional response dispatch.
//! - Server Transaction tracking: absorption and retransmission of responses for duplicate incoming requests.
//! - Message demultiplexing: responses are routed only to their matching transaction channel,
//!   and incoming unsolicited requests are routed to the incoming request queue without racing.

use crate::sip::transport::Transport;
use crate::sip::utils;
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};

// ── RFC 3261 §17 Timer Constants ─────────────────────────────────────────────

/// Default RTT estimate (T1) in milliseconds: 500ms
pub const T1_MS: u64 = 500;

/// Maximum retransmit interval for non-INVITE and INVITE (T2) in milliseconds: 4000ms
pub const T2_MS: u64 = 4000;

/// Maximum duration a message remains in the network (T4) in milliseconds: 5000ms
pub const T4_MS: u64 = 5000;

/// Client INVITE transaction timeout (Timer B): 64 * T1 = 32000ms
pub const TIMER_B_MS: u64 = 64 * T1_MS;

/// Client Non-INVITE transaction timeout (Timer F): 64 * T1 = 32000ms
pub const TIMER_F_MS: u64 = 64 * T1_MS;

/// Wait time for response retransmissions (Timer D): 32000ms for UDP, 0ms for TCP/TLS
pub const TIMER_D_MS: u64 = 32000;

/// Wait time for response retransmits (Timer K): T4 = 5000ms for UDP, 0ms for TCP/TLS
pub const TIMER_K_MS: u64 = T4_MS;

/// Server transaction wait time for request retransmissions (Timer J): 64 * T1 = 32000ms
#[allow(dead_code)]
pub const TIMER_J_MS: u64 = 64 * T1_MS;

// ── Transaction Key & States ──────────────────────────────────────────────────

/// Unique identifier for a SIP transaction (RFC 3261 §17.1.3).
///
/// Transactions are keyed on:
/// 1. The `branch` parameter of the topmost `Via` header (must start with `z9hG4bK`).
/// 2. The `method` from the `CSeq` header.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TransactionKey {
    pub branch: String,
    pub method: String,
}

impl std::fmt::Display for TransactionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{} {}]", self.method, self.branch)
    }
}

impl TransactionKey {
    pub fn new(branch: impl Into<String>, method: impl Into<String>) -> Self {
        Self {
            branch: branch.into().trim().to_string(),
            method: method.into().trim().to_ascii_uppercase(),
        }
    }

    /// Extract transaction key from a SIP message (request or response).
    ///
    /// Looks for the topmost Via header's `branch` parameter and the CSeq header's method.
    pub fn from_message(msg: &str) -> Option<Self> {
        let branch = utils::extract_param(msg, "Via", "branch");
        if branch.is_empty() {
            return None;
        }

        let cseq_hdr = utils::extract_header(msg, "CSeq");
        let method = cseq_hdr.split_whitespace().nth(1)?;

        Some(Self::new(branch, method))
    }

    /// Returns true if the branch parameter has the RFC 3261 magic cookie `z9hG4bK`.
    #[allow(dead_code)]
    pub fn is_rfc3261(&self) -> bool {
        self.branch.starts_with("z9hG4bK")
    }
}

/// Lifecycle states of a SIP Transaction (RFC 3261 §17.1 & §17.2).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransactionState {
    /// Client INVITE initial state: request sent, waiting for 1xx or final response
    Calling,
    /// Client Non-INVITE initial state: request sent, waiting for response
    Trying,
    /// Received provisional response (1xx)
    Proceeding,
    /// Received final response (2xx-6xx)
    Completed,
    /// Transaction finished and resources freed
    Terminated,
}

/// An incoming unsolicited SIP request (e.g. incoming INVITE, BYE, CANCEL).
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct IncomingRequest {
    pub method: String,
    pub raw: String,
    pub src: SocketAddr,
    pub key: TransactionKey,
}

/// Record of a server transaction for duplicate request filtering.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct ServerTransaction {
    pub key: TransactionKey,
    pub last_response: Option<String>,
    pub state: TransactionState,
}

// ── Transaction Manager ───────────────────────────────────────────────────────

/// Coordinates all active client transactions, server transactions, and incoming requests.
pub struct TransactionManager {
    /// Active client transactions waiting for responses: `(branch, method) -> sender`
    client_txs: Arc<Mutex<HashMap<TransactionKey, mpsc::UnboundedSender<String>>>>,
    /// Server transactions for absorbing duplicate requests: `(branch, method) -> ServerTransaction`
    server_txs: Arc<Mutex<HashMap<TransactionKey, ServerTransaction>>>,
    /// Completed client transactions absorbing late duplicate responses (Timer K / Timer D)
    completed_client_keys: Arc<Mutex<HashSet<TransactionKey>>>,
    /// Channel for delivering incoming unsolicited requests to watchers
    incoming_requests_tx: mpsc::UnboundedSender<IncomingRequest>,
    /// Channel receiver for incoming requests
    incoming_requests_rx: Arc<Mutex<mpsc::UnboundedReceiver<IncomingRequest>>>,
}

impl Default for TransactionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TransactionManager {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            client_txs: Arc::new(Mutex::new(HashMap::new())),
            server_txs: Arc::new(Mutex::new(HashMap::new())),
            completed_client_keys: Arc::new(Mutex::new(HashSet::new())),
            incoming_requests_tx: tx,
            incoming_requests_rx: Arc::new(Mutex::new(rx)),
        }
    }

    /// Process a raw received packet from the transport.
    ///
    /// - If it is a SIP response (`SIP/2.0 ...`), routes to the matching client transaction channel,
    ///   or absorbs if the transaction is in Completed state.
    /// - If it is a SIP request (`INVITE ...`, `BYE ...`), checks for retransmissions. If it is a
    ///   retransmitted request already answered, re-sends the cached response. If it is `OPTIONS`,
    ///   auto-replies with 200 OK. Otherwise, enqueues to `incoming_requests`.
    pub async fn process_incoming(&self, transport: &Transport, data: &[u8], src: SocketAddr) {
        let msg = String::from_utf8_lossy(data);
        let trimmed = msg.trim_start();
        if trimmed.is_empty() {
            return;
        }

        if trimmed.starts_with("SIP/2.0 ") {
            // ── Response Handling ───────────────────────────────────────────
            if let Some(key) = TransactionKey::from_message(trimmed) {
                let sender = {
                    let map = self.client_txs.lock().await;
                    map.get(&key).cloned()
                };

                if let Some(tx) = sender {
                    if let Err(e) = tx.send(trimmed.to_string()) {
                        log::debug!("Client transaction {} channel closed: {}", key, e);
                    }
                } else {
                    let is_completed = {
                        let completed = self.completed_client_keys.lock().await;
                        completed.contains(&key)
                    };
                    if is_completed {
                        log::debug!(
                            "Absorbed duplicate response for completed transaction {}",
                            key
                        );
                    } else {
                        log::debug!("Received unmatched response for transaction {}", key);
                    }
                }
            } else {
                log::debug!("Received malformed SIP response without valid Via branch / CSeq");
            }
        } else {
            // ── Request Handling ────────────────────────────────────────────
            let first_line = trimmed.lines().next().unwrap_or("");
            let method = first_line
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_uppercase();

            let branch = utils::extract_param(trimmed, "Via", "branch");
            let key = TransactionKey::new(branch, &method);

            // 1. Check for retransmitted request already answered by server transaction
            {
                let server_map = self.server_txs.lock().await;
                if let Some(st) = server_map.get(&key) {
                    if let Some(ref cached_resp) = st.last_response {
                        log::debug!(
                            "Retransmitting cached server response for duplicate request {}",
                            key
                        );
                        let _ = transport.send_to(cached_resp.as_bytes(), src).await;
                        return;
                    }
                }
            }

            // 2. Handle server OPTIONS keep-alive ping automatically
            if method == "OPTIONS" {
                log::debug!("Handling incoming OPTIONS keep-alive ping from {}", src);
                if let Some(options_resp) = build_options_200_ok(trimmed) {
                    let _ = transport.send_to(options_resp.as_bytes(), src).await;
                    return;
                }
            }

            // 3. Handle incoming PRACK request automatically (RFC 3262 §3.2)
            if method == "PRACK" {
                log::debug!("Handling incoming PRACK request from {}", src);
                if let Some(prack_resp) = build_prack_200_ok(trimmed) {
                    let _ = transport.send_to(prack_resp.as_bytes(), src).await;
                    return;
                }
            }

            // 4. Deliver request to incoming request queue
            let incoming = IncomingRequest {
                method,
                raw: trimmed.to_string(),
                src,
                key,
            };

            if let Err(e) = self.incoming_requests_tx.send(incoming) {
                log::error!("Failed to enqueue incoming SIP request: {}", e);
            }
        }
    }

    /// Record a response sent by the server for an incoming request,
    /// so that any future duplicate request can be answered automatically.
    #[allow(dead_code)]
    pub async fn record_server_response(
        &self,
        key: TransactionKey,
        response: String,
        is_reliable: bool,
    ) {
        let mut server_map = self.server_txs.lock().await;
        server_map.insert(
            key.clone(),
            ServerTransaction {
                key: key.clone(),
                last_response: Some(response),
                state: TransactionState::Completed,
            },
        );

        if !is_reliable {
            let server_txs = self.server_txs.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(TIMER_J_MS)).await;
                let mut map = server_txs.lock().await;
                map.remove(&key);
            });
        }
    }

    /// Try to receive the next queued incoming unsolicited request with a timeout.
    #[allow(dead_code)]
    pub async fn recv_incoming_request(&self, timeout_ms: u64) -> Option<IncomingRequest> {
        let mut rx = self.incoming_requests_rx.lock().await;
        match tokio::time::timeout(Duration::from_millis(timeout_ms), rx.recv()).await {
            Ok(Some(req)) => Some(req),
            _ => None,
        }
    }

    /// Try to pop an incoming request immediately without waiting.
    pub fn try_pop_incoming_request(&self) -> Option<IncomingRequest> {
        if let Ok(mut rx) = self.incoming_requests_rx.try_lock() {
            rx.try_recv().ok()
        } else {
            None
        }
    }

    /// Execute a Client Non-INVITE Transaction (RFC 3261 §17.1.2)
    ///
    /// Used for: REGISTER, BYE, INFO, REFER, MESSAGE, SUBSCRIBE, OPTIONS.
    ///
    /// - Starts in `Trying` state.
    /// - If UDP (unreliable transport): Retransmits with Timer E (500ms, doubling up to 4s).
    /// - Times out with Timer F (32s).
    /// - On 1xx: transitions to `Proceeding` (Timer E set to T2=4s).
    /// - On 2xx-6xx: transitions to `Completed`. If UDP, starts Timer K (5s) to absorb duplicates.
    pub async fn execute_non_invite(
        &self,
        transport: &Transport,
        target: SocketAddr,
        request: &str,
    ) -> Result<String> {
        let key = TransactionKey::from_message(request)
            .context("Cannot extract transaction key (Via branch / CSeq) from SIP request")?;

        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        {
            let mut map = self.client_txs.lock().await;
            map.insert(key.clone(), tx);
        }

        let is_reliable = transport.via_str() != "UDP";
        let mut state = TransactionState::Trying;

        // Send initial request
        crate::service::logger::record_sip_trace("OUT", "client", request, transport.via_str());
        transport.send_to(request.as_bytes(), target).await?;

        let mut timer_e_ms = T1_MS;
        let timer_f = tokio::time::sleep(Duration::from_millis(TIMER_F_MS));
        tokio::pin!(timer_f);

        loop {
            let timer_e = if !is_reliable {
                Some(tokio::time::sleep(Duration::from_millis(timer_e_ms)))
            } else {
                None
            };

            tokio::select! {
                _ = &mut timer_f => {
                    self.remove_client_transaction(&key).await;
                    anyhow::bail!("Non-INVITE transaction {} timed out (Timer F)", key);
                }
                _ = async {
                    if let Some(t) = timer_e {
                        t.await;
                    } else {
                        futures_util::future::pending::<()>().await;
                    }
                } => {
                    // Timer E fired -> retransmit
                    log::debug!("Timer E fired for {}, retransmitting Non-INVITE...", key);
                    crate::service::logger::record_sip_trace(
                        "OUT (RETRANSMIT)",
                        "client",
                        request,
                        transport.via_str(),
                    );
                    let _ = transport.send_to(request.as_bytes(), target).await;
                    if state == TransactionState::Trying {
                        timer_e_ms = (timer_e_ms * 2).min(T2_MS);
                    } else {
                        timer_e_ms = T2_MS;
                    }
                }
                resp_opt = rx.recv() => {
                    match resp_opt {
                        Some(resp) => {
                            let status = utils::parse_status_code(&resp)?;
                            crate::service::logger::record_sip_trace(
                                "IN",
                                "client",
                                &resp,
                                transport.via_str(),
                            );

                            if (100..200).contains(&status) {
                                state = TransactionState::Proceeding;
                                timer_e_ms = T2_MS;
                                // Keep waiting for final response
                            } else {
                                // Final response (2xx - 6xx)
                                self.remove_client_transaction(&key).await;
                                if !is_reliable {
                                    self.start_timer_k(key.clone(), TIMER_K_MS);
                                }
                                return Ok(resp);
                            }
                        }
                        None => {
                            self.remove_client_transaction(&key).await;
                            anyhow::bail!("Transaction {} response channel closed unexpectedly", key);
                        }
                    }
                }
                // Also poll transport cooperatively if no background receiver is running
                packet = transport.try_recv(100) => {
                    if let Some(data) = packet {
                        self.process_incoming(transport, &data, target).await;
                    }
                }
            }
        }
    }

    /// Execute a Client INVITE Transaction (RFC 3261 §17.1.1)
    ///
    /// - Starts in `Calling` state.
    /// - If UDP: Retransmits with Timer A (500ms, doubling each retransmission).
    /// - Times out with Timer B (32s).
    /// - On 1xx: stops Timer A, enters `Proceeding`, yields provisional response to callback.
    /// - On 2xx: transitions to `Terminated` immediately. UAC dialog layer sends ACK.
    /// - On 3xx-6xx: sends ACK for failure response, enters `Completed`, starts Timer D.
    pub async fn execute_invite<F>(
        &self,
        transport: &Transport,
        target: SocketAddr,
        request: &str,
        mut on_provisional: F,
    ) -> Result<String>
    where
        F: FnMut(u16, &str),
    {
        let key = TransactionKey::from_message(request)
            .context("Cannot extract transaction key (Via branch / CSeq) from INVITE request")?;

        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        {
            let mut map = self.client_txs.lock().await;
            map.insert(key.clone(), tx);
        }

        let is_reliable = transport.via_str() != "UDP";
        let mut state = TransactionState::Calling;

        crate::service::logger::record_sip_trace("OUT", "client", request, transport.via_str());
        transport.send_to(request.as_bytes(), target).await?;

        let mut timer_a_ms = T1_MS;
        let timer_b = tokio::time::sleep(Duration::from_millis(TIMER_B_MS));
        tokio::pin!(timer_b);

        loop {
            let timer_a = if !is_reliable && state == TransactionState::Calling {
                Some(tokio::time::sleep(Duration::from_millis(timer_a_ms)))
            } else {
                None
            };

            tokio::select! {
                _ = &mut timer_b => {
                    self.remove_client_transaction(&key).await;
                    anyhow::bail!("INVITE transaction {} timed out (Timer B)", key);
                }
                _ = async {
                    if let Some(t) = timer_a {
                        t.await;
                    } else {
                        futures_util::future::pending::<()>().await;
                    }
                } => {
                    // Timer A fired -> retransmit INVITE
                    log::debug!("Timer A fired for {}, retransmitting INVITE...", key);
                    crate::service::logger::record_sip_trace(
                        "OUT (RETRANSMIT)",
                        "client",
                        request,
                        transport.via_str(),
                    );
                    let _ = transport.send_to(request.as_bytes(), target).await;
                    timer_a_ms *= 2;
                }
                resp_opt = rx.recv() => {
                    match resp_opt {
                        Some(resp) => {
                            let status = utils::parse_status_code(&resp)?;
                            crate::service::logger::record_sip_trace(
                                "IN",
                                "client",
                                &resp,
                                transport.via_str(),
                            );

                            if (100..200).contains(&status) {
                                state = TransactionState::Proceeding;
                                on_provisional(status, &resp);

                                // RFC 3262: If provisional response is reliable (RSeq present), send PRACK
                                if let Some(rseq) = utils::parse_rseq(&resp) {
                                    let cseq_num = utils::extract_header(request, "CSeq")
                                        .split_whitespace()
                                        .next()
                                        .and_then(|s| s.parse::<u32>().ok())
                                        .unwrap_or(1);
                                    if let Some(prack) =
                                        build_prack_ack(request, &resp, rseq, cseq_num)
                                    {
                                        log::info!(
                                            "Sending PRACK for reliable provisional response (RSeq={})",
                                            rseq
                                        );
                                        crate::service::logger::record_sip_trace(
                                            "OUT (PRACK)",
                                            "client",
                                            &prack,
                                            transport.via_str(),
                                        );
                                        let _ = transport.send_to(prack.as_bytes(), target).await;
                                    }
                                }
                            } else if (200..300).contains(&status) {
                                // 2xx: Transaction terminates immediately (RFC 3261 §17.1.1.2)
                                self.remove_client_transaction(&key).await;
                                return Ok(resp);
                            } else {
                                // 3xx - 6xx: Send ACK per RFC 3261 §17.1.1.3, enter Completed
                                self.remove_client_transaction(&key).await;
                                if let Some(ack) = build_transaction_ack(request, &resp) {
                                    let _ = transport.send_to(ack.as_bytes(), target).await;
                                }
                                if !is_reliable {
                                    self.start_timer_k(key.clone(), TIMER_D_MS);
                                }
                                return Ok(resp);
                            }
                        }
                        None => {
                            self.remove_client_transaction(&key).await;
                            anyhow::bail!("INVITE transaction {} response channel closed unexpectedly", key);
                        }
                    }
                }
                packet = transport.try_recv(100) => {
                    if let Some(data) = packet {
                        self.process_incoming(transport, &data, target).await;
                    }
                }
            }
        }
    }

    /// Remove a client transaction from the active table.
    async fn remove_client_transaction(&self, key: &TransactionKey) {
        let mut map = self.client_txs.lock().await;
        map.remove(key);
    }

    /// Start Timer K (or Timer D) to absorb duplicate responses for a completed transaction.
    fn start_timer_k(&self, key: TransactionKey, duration_ms: u64) {
        let completed = self.completed_client_keys.clone();
        tokio::spawn(async move {
            {
                let mut set = completed.lock().await;
                set.insert(key.clone());
            }
            tokio::time::sleep(Duration::from_millis(duration_ms)).await;
            {
                let mut set = completed.lock().await;
                set.remove(&key);
            }
        });
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a 200 OK response for an incoming OPTIONS keep-alive request.
fn build_options_200_ok(req: &str) -> Option<String> {
    let via_headers = utils::extract_headers_raw(req, "Via");
    if via_headers.is_empty() {
        return None;
    }
    let via_block = via_headers.join("\r\n");
    let from = utils::extract_header(req, "From");
    let to = utils::extract_header(req, "To");
    let call_id = utils::extract_header(req, "Call-ID");
    let cseq = utils::extract_header(req, "CSeq");

    Some(format!(
        "SIP/2.0 200 OK\r\n\
         {}\r\n\
         From: {}\r\n\
         To: {}\r\n\
         Call-ID: {}\r\n\
         CSeq: {}\r\n\
         Allow: INVITE, ACK, CANCEL, OPTIONS, BYE, REFER, NOTIFY, MESSAGE, INFO\r\n\
         Accept: application/sdp\r\n\
         Content-Length: 0\r\n\
         \r\n",
        via_block, from, to, call_id, cseq
    ))
}

/// Build an automatic PRACK request acknowledging a reliable 1xx provisional response (RFC 3262 §3).
pub fn build_prack_ack(
    invite_req: &str,
    prov_resp: &str,
    rseq: u32,
    invite_cseq: u32,
) -> Option<String> {
    let ruri = utils::extract_header(prov_resp, "Contact")
        .chars()
        .filter(|&c| c != '<' && c != '>')
        .collect::<String>();
    let ruri = if !ruri.trim().is_empty() && ruri.starts_with("sip:") {
        ruri.trim().to_string()
    } else {
        invite_req
            .lines()
            .next()?
            .split_whitespace()
            .nth(1)?
            .to_string()
    };

    let via = utils::extract_header(invite_req, "Via");
    let from = utils::extract_header(invite_req, "From");
    let to = utils::extract_header(prov_resp, "To");
    let call_id = utils::extract_header(invite_req, "Call-ID");
    let prack_cseq = invite_cseq.wrapping_add(1);

    Some(format!(
        "PRACK {} SIP/2.0\r\n\
         Via: {}\r\n\
         Max-Forwards: 70\r\n\
         From: {}\r\n\
         To: {}\r\n\
         Call-ID: {}\r\n\
         CSeq: {} PRACK\r\n\
         RAck: {} {} INVITE\r\n\
         Content-Length: 0\r\n\
         \r\n",
        ruri, via, from, to, call_id, prack_cseq, rseq, invite_cseq
    ))
}

/// Build a 200 OK response for an incoming PRACK request (RFC 3262 §3.2).
pub fn build_prack_200_ok(req: &str) -> Option<String> {
    let via_headers = utils::extract_headers_raw(req, "Via");
    if via_headers.is_empty() {
        return None;
    }
    let via_block = via_headers.join("\r\n");
    let from = utils::extract_header(req, "From");
    let to = utils::extract_header(req, "To");
    let call_id = utils::extract_header(req, "Call-ID");
    let cseq = utils::extract_header(req, "CSeq");
    let rack = utils::extract_header(req, "RAck");
    let rack_header = if !rack.is_empty() {
        format!("RAck: {}\r\n", rack)
    } else {
        String::new()
    };

    Some(format!(
        "SIP/2.0 200 OK\r\n\
         {}\r\n\
         From: {}\r\n\
         To: {}\r\n\
         Call-ID: {}\r\n\
         CSeq: {}\r\n\
         {}\
         Content-Length: 0\r\n\
         \r\n",
        via_block, from, to, call_id, cseq, rack_header
    ))
}

/// Build an ACK request for an INVITE that received a 3xx-6xx failure response (RFC 3261 §17.1.1.3).
fn build_transaction_ack(invite_req: &str, resp: &str) -> Option<String> {
    let first_line = invite_req.lines().next()?;
    let ruri = first_line.split_whitespace().nth(1)?;

    let via = utils::extract_header(invite_req, "Via");
    let from = utils::extract_header(invite_req, "From");
    let to = utils::extract_header(resp, "To");
    let call_id = utils::extract_header(invite_req, "Call-ID");
    let cseq_num = utils::extract_header(invite_req, "CSeq")
        .split_whitespace()
        .next()?
        .to_string();

    Some(format!(
        "ACK {} SIP/2.0\r\n\
         Via: {}\r\n\
         Max-Forwards: 70\r\n\
         From: {}\r\n\
         To: {}\r\n\
         Call-ID: {}\r\n\
         CSeq: {} ACK\r\n\
         Content-Length: 0\r\n\
         \r\n",
        ruri, via, from, to, call_id, cseq_num
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_key_extraction() {
        let req = "REGISTER sip:sip.example.com SIP/2.0\r\n\
                   Via: SIP/2.0/UDP 192.168.1.50:5060;branch=z9hG4bK-776asdhds\r\n\
                   Max-Forwards: 70\r\n\
                   To: <sip:user@example.com>\r\n\
                   From: <sip:user@example.com>;tag=1928301774\r\n\
                   Call-ID: a84b4c76e66710\r\n\
                   CSeq: 314159 REGISTER\r\n\r\n";

        let key = TransactionKey::from_message(req).expect("valid key");
        assert_eq!(key.branch, "z9hG4bK-776asdhds");
        assert_eq!(key.method, "REGISTER");
        assert!(key.is_rfc3261());

        let resp = "SIP/2.0 200 OK\r\n\
                    Via: SIP/2.0/UDP 192.168.1.50:5060;branch=z9hG4bK-776asdhds;received=192.0.2.1\r\n\
                    To: <sip:user@example.com>;tag=99as8\r\n\
                    From: <sip:user@example.com>;tag=1928301774\r\n\
                    Call-ID: a84b4c76e66710\r\n\
                    CSeq: 314159 REGISTER\r\n\r\n";

        let resp_key = TransactionKey::from_message(resp).expect("valid resp key");
        assert_eq!(key, resp_key);
    }

    #[test]
    fn test_transaction_key_matches_invite_response() {
        let req = "INVITE sip:bob@example.com SIP/2.0\r\n\
                   Via: SIP/2.0/UDP 192.168.1.50:5060;branch=z9hG4bK-invite123\r\n\
                   CSeq: 1 INVITE\r\n\r\n";
        let resp_trying = "SIP/2.0 100 Trying\r\n\
                           Via: SIP/2.0/UDP 192.168.1.50:5060;branch=z9hG4bK-invite123\r\n\
                           CSeq: 1 INVITE\r\n\r\n";
        let resp_ok = "SIP/2.0 200 OK\r\n\
                       Via: SIP/2.0/UDP 192.168.1.50:5060;branch=z9hG4bK-invite123\r\n\
                       CSeq: 1 INVITE\r\n\r\n";

        let k_req = TransactionKey::from_message(req).unwrap();
        let k_100 = TransactionKey::from_message(resp_trying).unwrap();
        let k_200 = TransactionKey::from_message(resp_ok).unwrap();

        assert_eq!(k_req, k_100);
        assert_eq!(k_req, k_200);
    }

    #[test]
    fn test_build_options_200_ok() {
        let req = "OPTIONS sip:192.168.1.50:5060 SIP/2.0\r\n\
                   Via: SIP/2.0/UDP 10.0.0.1:5060;branch=z9hG4bK-ping1\r\n\
                   From: <sip:pbx@10.0.0.1>;tag=pingtag\r\n\
                   To: <sip:192.168.1.50:5060>\r\n\
                   Call-ID: ping12345@10.0.0.1\r\n\
                   CSeq: 102 OPTIONS\r\n\r\n";

        let resp = build_options_200_ok(req).expect("options 200 ok");
        assert!(resp.starts_with("SIP/2.0 200 OK\r\n"));
        assert!(resp.contains("branch=z9hG4bK-ping1"));
        assert!(resp.contains("CSeq: 102 OPTIONS"));
    }

    #[test]
    fn test_build_transaction_ack() {
        let req = "INVITE sip:bob@example.com SIP/2.0\r\n\
                   Via: SIP/2.0/UDP 192.168.1.50:5060;branch=z9hG4bK-failbranch\r\n\
                   From: <sip:alice@example.com>;tag=atag\r\n\
                   To: <sip:bob@example.com>\r\n\
                   Call-ID: callid123\r\n\
                   CSeq: 1 INVITE\r\n\r\n";

        let resp = "SIP/2.0 486 Busy Here\r\n\
                    Via: SIP/2.0/UDP 192.168.1.50:5060;branch=z9hG4bK-failbranch\r\n\
                    From: <sip:alice@example.com>;tag=atag\r\n\
                    To: <sip:bob@example.com>;tag=btag\r\n\
                    Call-ID: callid123\r\n\
                    CSeq: 1 INVITE\r\n\r\n";

        let ack = build_transaction_ack(req, resp).expect("ack built");
        assert!(ack.starts_with("ACK sip:bob@example.com SIP/2.0\r\n"));
        assert!(ack.contains("branch=z9hG4bK-failbranch"));
        assert!(ack.contains("To: <sip:bob@example.com>;tag=btag"));
        assert!(ack.contains("CSeq: 1 ACK"));
    }

    #[test]
    fn test_build_prack_ack() {
        let req = "INVITE sip:bob@example.com SIP/2.0\r\n\
                   Via: SIP/2.0/UDP 192.168.1.50:5060;branch=z9hG4bK-invite1\r\n\
                   From: <sip:alice@example.com>;tag=atag\r\n\
                   To: <sip:bob@example.com>\r\n\
                   Call-ID: callid1\r\n\
                   CSeq: 1 INVITE\r\n\r\n";

        let prov_resp = "SIP/2.0 183 Session Progress\r\n\
                         Via: SIP/2.0/UDP 192.168.1.50:5060;branch=z9hG4bK-invite1\r\n\
                         From: <sip:alice@example.com>;tag=atag\r\n\
                         To: <sip:bob@example.com>;tag=btag\r\n\
                         Call-ID: callid1\r\n\
                         CSeq: 1 INVITE\r\n\
                         Contact: <sip:bob@192.168.1.60:5060>\r\n\
                         RSeq: 42\r\n\r\n";

        let prack = build_prack_ack(req, prov_resp, 42, 1).expect("prack built");
        assert!(prack.starts_with("PRACK sip:bob@192.168.1.60:5060 SIP/2.0\r\n"));
        assert!(prack.contains("RAck: 42 1 INVITE"));
        assert!(prack.contains("CSeq: 2 PRACK"));
    }

    #[test]
    fn test_build_prack_200_ok() {
        let prack_req = "PRACK sip:bob@192.168.1.60:5060 SIP/2.0\r\n\
                         Via: SIP/2.0/UDP 192.168.1.50:5060;branch=z9hG4bK-prack1\r\n\
                         From: <sip:alice@example.com>;tag=atag\r\n\
                         To: <sip:bob@example.com>;tag=btag\r\n\
                         Call-ID: callid1\r\n\
                         CSeq: 2 PRACK\r\n\
                         RAck: 42 1 INVITE\r\n\r\n";

        let resp = build_prack_200_ok(prack_req).expect("prack 200 ok");
        assert!(resp.starts_with("SIP/2.0 200 OK\r\n"));
        assert!(resp.contains("RAck: 42 1 INVITE"));
        assert!(resp.contains("CSeq: 2 PRACK"));
    }
}
