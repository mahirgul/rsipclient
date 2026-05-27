mod handlers;
pub(crate) mod logger;
pub(crate) mod watcher;
pub(crate) mod web_server;

pub(crate) use watcher::incoming_call_watcher;

use crate::config::{Account, Config};
use crate::ipc::{Request, Response};
use crate::rtp::codec::Codec;
use crate::sip::{AuthMethod, SipClient, SipSettings};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

/// Wrapper for a managed SIP client (one per account)
pub(crate) struct ManagedClient {
    pub account: Account,
    pub client: Arc<Mutex<SipClient>>,
    pub codec: Codec,
    pub active: Arc<Mutex<bool>>,
}

/// The service that holds all managed clients and handles IPC
pub struct Service {
    pub(crate) clients: Arc<Mutex<HashMap<String, ManagedClient>>>,
    pub(crate) control_port: u16,
    pub(crate) config_path: String,
    pub(crate) web_port: u16,
    pub(crate) web_username: String,
    pub(crate) web_password: String,
    pub(crate) global_shutdown: Arc<Mutex<bool>>,
}

/// Helper to build a SipClient and wrap it in a ManagedClient
pub(crate) async fn create_managed_client(account: &Account) -> Result<ManagedClient> {
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
    .await?;

    Ok(ManagedClient {
        account: account.clone(),
        client: Arc::new(Mutex::new(client)),
        codec,
        active: Arc::new(Mutex::new(true)),
    })
}

impl Service {
    /// Create the service, initializing all accounts from config
    pub async fn new(config: &Config, control_port: u16, config_path: String) -> Result<Self> {
        let mut clients = HashMap::new();

        for account in &config.accounts {
            match create_managed_client(account).await {
                Ok(mc) => {
                    log::info!(
                        "Account '{}' ready — bound to {}, RTP {}-{}",
                        account.name,
                        mc.client.lock().await.local_addr,
                        account.rtp_port_start,
                        account.rtp_port_end
                    );
                    clients.insert(account.name.clone(), mc);
                }
                Err(e) => {
                    log::error!("Failed to create client for '{}': {}", account.name, e);
                }
            }
        }

        let (web_port, web_username, web_password) = if let Some(ref web) = config.web {
            (web.port, web.username.clone(), web.password.clone())
        } else {
            (9090, "admin".to_string(), "admin".to_string())
        };

        Ok(Service {
            clients: Arc::new(Mutex::new(clients)),
            control_port,
            config_path,
            web_port,
            web_username,
            web_password,
            global_shutdown: Arc::new(Mutex::new(false)),
        })
    }

    /// Start the control listener and web server — blocks until shutdown
    pub async fn run(self) -> Result<()> {
        let bind_addr = format!("127.0.0.1:{}", self.control_port);
        let listener = TcpListener::bind(&bind_addr)
            .await
            .context(format!("Failed to bind control port {}", bind_addr))?;

        let clients = self.clients.clone();
        let shutdown = self.global_shutdown.clone();

        println!(
            "Service running on {} (control port {})",
            bind_addr, self.control_port
        );
        
        // Spawn incoming-call watchers for auto-answer accounts
        {
            let cls = clients.lock().await;
            println!(
                "Accounts: {}",
                cls.keys().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            );
            for (name, mc) in cls.iter() {
                if mc.account.auto_answer.unwrap_or(false) {
                    let client = mc.client.clone();
                    let codec = mc.codec;
                    let account = mc.account.clone();
                    let shutdown = shutdown.clone();
                    let active = mc.active.clone();
                    let account_name = name.clone();
                    log::info!("Auto-answer enabled for '{}'", account_name);

                    tokio::spawn(async move {
                        incoming_call_watcher(account_name, client, codec, account, shutdown, active).await;
                    });
                }
            }
        }

        // Spawn Web Dashboard server
        let web_state = web_server::AppState {
            clients: self.clients.clone(),
            global_shutdown: shutdown.clone(),
            config_path: self.config_path.clone(),
            web_username: self.web_username.clone(),
            web_password: self.web_password.clone(),
            session_token: uuid::Uuid::new_v4().to_string(),
            start_time: std::time::Instant::now(),
        };
        let web_port = self.web_port;
        tokio::spawn(async move {
            web_server::start_web_server(web_state, web_port).await;
        });

        println!("Send 'shutdown' command to stop, or access Web Dashboard at http://localhost:{}", self.web_port);

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
        clients: Arc<Mutex<HashMap<String, ManagedClient>>>,
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
        let resp = {
            let cls = clients.lock().await;
            handlers::process_command(&req, &cls).await
        };

        let json = format!("{}\n", serde_json::to_string(&resp).unwrap());
        let _ = write_half.write_all(json.as_bytes()).await;

        if is_shutdown {
            *shutdown.lock().await = true;
        }
    }
}


