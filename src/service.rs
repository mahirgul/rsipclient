//! Service module — manages SipClient instances, listens for IPC commands

mod handlers;

use crate::config::{Account, Config};
use crate::ipc::{Request, Response};
use crate::ivr;
use crate::rtp::codec::Codec;
use crate::rtp::receiver::RtpReceiver;
use crate::sip::{sdp, utils, AuthMethod, SipClient, SipSettings};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

/// Wrapper for a managed SIP client (one per account)
pub(crate) struct ManagedClient {
    #[allow(dead_code)]
    pub account: Account,
    pub client: Arc<Mutex<SipClient>>,
    pub codec: Codec,
}

/// The service that holds all managed clients and handles IPC
pub struct Service {
    clients: HashMap<String, ManagedClient>,
    control_port: u16,
}

impl Service {
    /// Create the service, initializing all accounts from config
    pub async fn new(config: &Config, control_port: u16) -> Result<Self> {
        let mut clients = HashMap::new();

        for account in &config.accounts {
            let server_addr: SocketAddr = account
                .server
                .parse()
                .context(format!("Invalid server address for '{}'", account.name))?;

            let local_addr: SocketAddr = format!("0.0.0.0:{}", account.sip_port)
                .parse()
                .context(format!("Invalid local address for '{}'", account.name))?;

            let auth_method = match account.auth_method.as_deref() {
                Some("none") | Some("None") => AuthMethod::None,
                _ => AuthMethod::Md5,
            };

            let codec =
                Codec::from_str(account.codec.as_deref().unwrap_or("pcmu")).unwrap_or(Codec::Pcmu);

            let sip_settings = SipSettings::from_config(
                account.display_name.clone(),
                account.asserted_id.clone(),
                account.preferred_id.clone(),
                account.proxy.clone(),
                account.register_expiry,
                account.user_agent.clone(),
                account.dtmf_mode.clone(),
                account.early_media,
                account.session_timers,
            );

            let client = SipClient::new(
                server_addr,
                local_addr,
                account.username.clone(),
                account.password.clone(),
                account.domain.clone(),
                account.rtp_port_start,
                account.rtp_port_end,
                auth_method,
                sip_settings,
            )
            .await
            .context(format!("Failed to create client for '{}'", account.name))?;

            log::info!(
                "Account '{}' ready — bound to {}, RTP {}-{}",
                account.name,
                client.local_addr,
                account.rtp_port_start,
                account.rtp_port_end
            );

            clients.insert(
                account.name.clone(),
                ManagedClient {
                    account: account.clone(),
                    client: Arc::new(Mutex::new(client)),
                    codec,
                },
            );
        }

        Ok(Service {
            clients,
            control_port,
        })
    }

    /// Start the control listener — blocks until shutdown
    pub async fn run(self) -> Result<()> {
        let bind_addr = format!("127.0.0.1:{}", self.control_port);
        let listener = TcpListener::bind(&bind_addr)
            .await
            .context(format!("Failed to bind control port {}", bind_addr))?;

        let clients = Arc::new(self.clients);
        let shutdown = Arc::new(Mutex::new(false));

        println!(
            "Service running on {} (control port {})",
            bind_addr, self.control_port
        );
        println!(
            "Accounts: {}",
            clients
                .keys()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        println!("Send 'shutdown' command to stop.");

        // Spawn incoming-call watchers for auto-answer accounts
        for (name, mc) in clients.iter() {
            if mc.account.auto_answer.unwrap_or(false) {
                let client = mc.client.clone();
                let codec = mc.codec;
                let account = mc.account.clone();
                let shutdown = shutdown.clone();
                let account_name = name.clone();
                log::info!("Auto-answer enabled for '{}'", account_name);

                tokio::spawn(async move {
                    incoming_call_watcher(account_name, client, codec, account, shutdown).await;
                });
            }
        }

        loop {
            if *shutdown.lock().await {
                println!("Shutting down service.");
                break;
            }

            let accept_result =
                tokio::time::timeout(std::time::Duration::from_millis(500), listener.accept())
                    .await;

            match accept_result {
                Ok(Ok((stream, addr))) => {
                    log::debug!("Control connection from {}", addr);
                    tokio::spawn({
                        let clients = clients.clone();
                        let shutdown = shutdown.clone();
                        async move {
                            Self::handle_connection(stream, clients, shutdown).await;
                        }
                    });
                }
                Ok(Err(e)) => log::error!("Accept error: {}", e),
                Err(_) => { /* timeout — loop back to check shutdown flag */ }
            }
        }

        println!("Service stopped.");
        Ok(())
    }

    /// Handle one control connection: read → process → respond
    async fn handle_connection(
        stream: TcpStream,
        clients: Arc<HashMap<String, ManagedClient>>,
        shutdown: Arc<Mutex<bool>>,
    ) {
        let (reader_half, mut write_half) = stream.into_split();
        let mut buf_reader = BufReader::new(reader_half);
        let mut line = String::new();

        match buf_reader.read_line(&mut line).await {
            Ok(0) => return,
            Ok(_) => {}
            Err(e) => {
                let resp = Response::fail(&format!("Read error: {}", e));
                let _ = write_half
                    .write_all(format!("{}\n", serde_json::to_string(&resp).unwrap()).as_bytes())
                    .await;
                return;
            }
        };

        let req: Request = match serde_json::from_str(line.trim()) {
            Ok(r) => r,
            Err(e) => {
                let resp = Response::fail(&format!("Invalid JSON: {}", e));
                let _ = write_half
                    .write_all(format!("{}\n", serde_json::to_string(&resp).unwrap()).as_bytes())
                    .await;
                return;
            }
        };

        log::info!("Command: {:?}", req);

        let is_shutdown = req.cmd == "shutdown";
        let resp = handlers::process_command(&req, &clients).await;

        let json = format!("{}\n", serde_json::to_string(&resp).unwrap());
        let _ = write_half.write_all(json.as_bytes()).await;

        if is_shutdown {
            *shutdown.lock().await = true;
        }
    }
}

// ── Incoming call watcher ──────────────────────────────────

/// Background task: poll SIP socket for incoming INVITEs, auto-answer, run IVR.
async fn incoming_call_watcher(
    account_name: String,
    client: Arc<Mutex<SipClient>>,
    codec: Codec,
    account: Account,
    shutdown: Arc<Mutex<bool>>,
) {
    loop {
        if *shutdown.lock().await {
            break;
        }

        // Poll for incoming SIP message
        let msg = {
            let c = client.lock().await;
            c.try_recv(200).await
        };

        let msg = match msg {
            Some(m) => m,
            None => {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                continue;
            }
        };

        // Only handle INVITE
        if !msg.starts_with("INVITE") {
            continue;
        }

        log::info!("[{}] Incoming INVITE!", account_name);
        log::debug!("--- INCOMING ---\n{}", msg);

        // Extract Call-ID, From tag, To tag, Contact, and remote RTP from SDP
        let from_tag = utils::extract_param(&msg, "From", "tag");
        let call_id = utils::extract_header(&msg, "Call-ID");
        let cseq_str = utils::extract_header(&msg, "CSeq");
        let cseq: u32 = cseq_str
            .split_whitespace()
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let via_branch = utils::extract_param(&msg, "Via", "branch");

        // Parse remote RTP addr from SDP
        let remote_rtp = parse_sdp_connection(&msg);

        // Auto-answer: build 200 OK with SDP
        let response = {
            let c = client.lock().await;
            let local_ip = c.local_addr.ip().to_string();
            let sdp_body = sdp::build_sdp_default(&c.username, &local_ip, c.rtp_port_start);
            let sdp_len = sdp_body.len();

            format!(
                "SIP/2.0 200 OK\r\n\
                 Via: SIP/2.0/UDP {};branch={}\r\n\
                 From: <sip:{}@{}>;tag={}\r\n\
                 To: <sip:{}@{}>;tag={}\r\n\
                 Call-ID: {}\r\n\
                 CSeq: {} INVITE\r\n\
                 Contact: <sip:{}@{}>\r\n\
                 Content-Type: application/sdp\r\n\
                 Content-Length: {}\r\n\
                 \r\n\
                 {}",
                c.local_addr_str(),
                via_branch,
                c.username,
                c.domain,
                from_tag,
                c.username,
                c.domain,
                c.local_tag,
                call_id,
                cseq,
                c.username,
                c.local_addr_str(),
                sdp_len,
                sdp_body,
            )
        };

        // Send 200 OK
        {
            let c = client.lock().await;
            log::debug!("--- SEND 200 OK ---\n{}", response);
            let _ = c
                .transport
                .send_to(response.as_bytes(), c.server_addr)
                .await;
        }

        // Wait for ACK
        let ack_received = {
            let c = client.lock().await;
            match c.recv_extra(5000).await {
                Ok(ack) => ack.starts_with("ACK"),
                Err(_) => false,
            }
        };

        if !ack_received {
            log::warn!("[{}] No ACK received, skipping IVR", account_name);
            continue;
        }

        // Start RTP receiver
        let rtp_port = {
            let c = client.lock().await;
            c.rtp_port_start
        };
        let receiver = match RtpReceiver::bind(rtp_port).await {
            Ok(r) => r,
            Err(e) => {
                log::error!("[{}] Failed to bind RTP receiver: {}", e, account_name);
                continue;
            }
        };
        receiver.start();

        // Mark as in-call
        {
            let mut c = client.lock().await;
            c.in_call = true;
            c.call_id = Some(call_id.clone());
            c.remote_tag = Some(from_tag.clone());
        }

        // Run IVR if configured
        if let Some(ivr_config) = ivr::build_ivr_config(&account) {
            let remote_addr = match remote_rtp {
                Some(addr) => addr,
                None => {
                    log::warn!("[{}] No RTP address in SDP", account_name);
                    continue;
                }
            };

            log::info!("[{}] Starting IVR session", account_name);
            let session = ivr::IvrSession::new(ivr_config, codec);
            if let Err(e) = session.run(&client, remote_addr, &receiver).await {
                log::error!("[{}] IVR error: {}", e, account_name);
            }
        }

        // Cleanup call state
        {
            let mut c = client.lock().await;
            c.in_call = false;
            c.call_id = None;
            c.remote_tag = None;
        }
    }
}

/// Parse the first `c=` line from SDP body, return SocketAddr
fn parse_sdp_connection(msg: &str) -> Option<SocketAddr> {
    // Find c= line
    let c_line = msg.lines().find(|l| l.starts_with("c="))?;
    // c=IN IP4 192.168.1.1
    let parts: Vec<&str> = c_line.split_whitespace().collect();
    let ip = parts.get(2)?;

    // Find m= line for port
    let m_line = msg.lines().find(|l| l.starts_with("m=audio"))?;
    let m_parts: Vec<&str> = m_line.split_whitespace().collect();
    let port: u16 = m_parts.get(1)?.parse().ok()?;

    format!("{}:{}", ip, port).parse().ok()
}
