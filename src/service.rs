//! SIP service coordinator module.
//!
//! Exposes and coordinates background watchers, registration monitors,
//! the IPC server for CLI commands, and the web-based manager dashboard.

pub(crate) mod commands_server;
mod handlers;
pub(crate) mod logger;
pub(crate) mod managed_client;
pub(crate) mod watcher;
pub(crate) mod web_handlers;
pub(crate) mod web_server;

pub(crate) use managed_client::{create_managed_client, spawn_watchers_for_client, ManagedClient};
pub(crate) use watcher::{incoming_call_watcher, registration_watcher};

use crate::config::Config;
use crate::ipc::{Request, Response};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

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
    pub(crate) plugin_manager: crate::plugins::PluginManager,
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

        let plugin_config = config.plugins.clone().unwrap_or_default();
        let plugin_manager = crate::plugins::PluginManager::new(plugin_config);

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
            plugin_manager,
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
                spawn_watchers_for_client(name.clone(), mc, shutdown.clone());
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
            plugin_manager: self.plugin_manager.clone(),
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
