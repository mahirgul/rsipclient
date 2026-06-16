//! SIP service coordinator module.
//!
//! Exposes and coordinates background watchers, registration monitors,
//! the IPC server for CLI commands, and the web-based manager dashboard.

pub(crate) mod commands_server;
mod handlers;
pub(crate) mod logger;
pub(crate) mod watcher;
pub(crate) mod web_handlers;
pub(crate) mod web_server;

pub(crate) use watcher::{incoming_call_watcher, registration_watcher};

use crate::config::{Account, Config};
use crate::ipc::{Request, Response};
use crate::rtp::codec::Codec;
use crate::sip::transport::Transport;
use crate::sip::{AuthMethod, SipClient, SipSettings};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

/// Wrapper for a managed SIP client (one per account)
#[derive(Clone)]
pub(crate) struct ManagedClient {
    pub account: Account,
    pub client: Arc<Mutex<SipClient>>,
    pub codec: Codec,
    pub active: Arc<Mutex<bool>>,
    pub should_register: Arc<Mutex<bool>>,
    pub audio_tx: tokio::sync::broadcast::Sender<Vec<i16>>,
}

/// The service that holds all managed clients and handles IPC
pub struct Service {
    pub(crate) clients: Arc<Mutex<HashMap<String, ManagedClient>>>,
    pub(crate) control_port: u16,
    pub(crate) config_path: String,
    pub(crate) web_port: u16,
    pub(crate) web_username: String,
    pub(crate) web_password: String,
    pub(crate) commands_port: u16,
    pub(crate) commands_username: Option<String>,
    pub(crate) commands_password: Option<String>,
    pub(crate) global_shutdown: Arc<Mutex<bool>>,
}

/// Helper to build a SipClient and wrap it in a ManagedClient
pub(crate) async fn create_managed_client(account: &Account) -> Result<ManagedClient> {
    let transport_type = account.transport.as_deref().unwrap_or("udp").to_lowercase();

    let default_port: u16 = if transport_type == "tls" { 5061 } else { 5060 };

    // Parse server address, auto-appending default port if missing
    let server_addr: SocketAddr = if account.server.contains(':') {
        account.server.parse().context(format!(
            "Invalid server address for '{}': {}",
            account.name, account.server
        ))?
    } else {
        format!("{}:{}", account.server, default_port)
            .parse()
            .context(format!(
                "Invalid server address for '{}': {}",
                account.name, account.server
            ))?
    };

    let (transport, local_addr) = if transport_type == "tls" {
        let bind_addr: SocketAddr = format!("0.0.0.0:{}", account.sip_port).parse()?;
        let transport = Transport::new_tls(bind_addr, server_addr, &account.domain).await?;
        let local_addr = transport.local_addr()?;
        log::info!(
            "Account '{}' using TLS transport to {}",
            account.name,
            server_addr
        );
        (transport, local_addr)
    } else if transport_type == "tcp" {
        let bind_addr: SocketAddr = format!("0.0.0.0:{}", account.sip_port).parse()?;
        let transport = Transport::new_tcp(bind_addr, server_addr).await?;
        let local_addr = transport.local_addr()?;
        log::info!(
            "Account '{}' using TCP transport to {}",
            account.name,
            server_addr
        );
        (transport, local_addr)
    } else {
        let bind_addr: SocketAddr = format!("0.0.0.0:{}", account.sip_port).parse()?;
        let transport = Transport::new_udp(bind_addr).await?;
        let local_addr = transport.local_addr()?;
        log::info!(
            "Account '{}' using UDP transport to {}",
            account.name,
            server_addr
        );
        (transport, local_addr)
    };

    let auth_method = match account.auth_method.as_deref() {
        Some("none") | Some("None") => AuthMethod::None,
        _ => AuthMethod::Md5,
    };

    let codec = Codec::from_str(account.codec.as_deref().unwrap_or("pcmu")).unwrap_or(Codec::Pcmu);

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
        transport,
        server_addr,
        local_addr,
        account.username.clone(),
        account.password.clone(),
        account.domain.clone(),
        account.rtp_port_start,
        account.rtp_port_end,
        auth_method,
        sip_settings,
        codec.to_config_str().to_string(),
    )
    .await?;

    let default_register = account.register_expiry.is_some();
    let (audio_tx, _) = tokio::sync::broadcast::channel(1000);

    Ok(ManagedClient {
        account: account.clone(),
        client: Arc::new(Mutex::new(client)),
        codec,
        active: Arc::new(Mutex::new(true)),
        should_register: Arc::new(Mutex::new(default_register)),
        audio_tx,
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

        let (commands_port, commands_username, commands_password) =
            if let Some(ref cmd_api) = config.commands_api {
                (
                    cmd_api.port,
                    cmd_api.username.clone(),
                    cmd_api.password.clone(),
                )
            } else {
                (9099, None, None)
            };

        Ok(Service {
            clients: Arc::new(Mutex::new(clients)),
            control_port,
            config_path,
            web_port,
            web_username,
            web_password,
            commands_port,
            commands_username,
            commands_password,
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

        // Spawn watchers for each account
        {
            let cls = clients.lock().await;
            println!(
                "Accounts: {}",
                cls.keys()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            for (name, mc) in cls.iter() {
                if mc.account.auto_answer.unwrap_or(false) {
                    let client = mc.client.clone();
                    let codec = mc.codec;
                    let account = mc.account.clone();
                    let shutdown = shutdown.clone();
                    let active = mc.active.clone();
                    let audio_tx = mc.audio_tx.clone();
                    let account_name = name.clone();
                    log::info!("Auto-answer enabled for '{}'", account_name);

                    tokio::spawn(async move {
                        incoming_call_watcher(
                            account_name,
                            client,
                            codec,
                            account,
                            shutdown,
                            active,
                            audio_tx,
                        )
                        .await;
                    });
                }

                // Spawn registration watcher
                let client = mc.client.clone();
                let active = mc.active.clone();
                let should_register = mc.should_register.clone();
                let register_expiry = mc.account.register_expiry.unwrap_or(3600);
                let retry_interval = mc.account.register_retry_interval.unwrap_or(30);
                let shutdown = shutdown.clone();
                let account_name = name.clone();

                tokio::spawn(async move {
                    registration_watcher(
                        account_name,
                        client,
                        active,
                        should_register,
                        register_expiry,
                        retry_interval,
                        shutdown,
                    )
                    .await;
                });
            }
        }

        // Initialize sysinfo System once at startup for dashboard metrics
        let mut sys = sysinfo::System::new_all();
        sys.refresh_all();

        // Spawn Web Dashboard server
        let web_state = web_server::AppState {
            clients: self.clients.clone(),
            global_shutdown: shutdown.clone(),
            config_path: self.config_path.clone(),
            web_username: self.web_username.clone(),
            web_password: self.web_password.clone(),
            session_token: uuid::Uuid::new_v4().to_string(),
            start_time: std::time::Instant::now(),
            sys: Arc::new(Mutex::new(sys)),
        };
        let web_port = self.web_port;
        tokio::spawn(async move {
            web_server::start_web_server(web_state, web_port).await;
        });

        // Spawn REST Commands server
        let cmd_state = commands_server::CommandsServerState {
            clients: self.clients.clone(),
            global_shutdown: shutdown.clone(),
            username: self.commands_username.clone(),
            password: self.commands_password.clone(),
            fallback_web_username: self.web_username.clone(),
            fallback_web_password: self.web_password.clone(),
        };
        let cmd_port = self.commands_port;
        tokio::spawn(async move {
            commands_server::start_commands_server(cmd_state, cmd_port).await;
        });

        println!(
            "Send 'shutdown' command to stop, or access Web Dashboard at http://localhost:{}",
            self.web_port
        );

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
            let cls = {
                let guard = clients.lock().await;
                guard.clone()
            };
            handlers::process_command(&req, &cls).await
        };

        let json = format!("{}\n", serde_json::to_string(&resp).unwrap());
        let _ = write_half.write_all(json.as_bytes()).await;

        if is_shutdown {
            *shutdown.lock().await = true;
        }
    }
}
