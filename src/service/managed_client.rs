//! Managed Client struct and constructor/helper functions.

use crate::config::Account;
use crate::rtp::codec::Codec;
use crate::service::watcher::{incoming_call_watcher, registration_watcher};
use crate::sip::transport::{TlsConfig, Transport};
use crate::sip::{AuthMethod, SipClient, SipSettings};
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Wrapper for a managed SIP client (one per account)
#[derive(Clone)]
pub struct ManagedClient {
    pub account: Account,
    pub client: Arc<Mutex<SipClient>>,
    pub codec: Codec,
    pub active: Arc<Mutex<bool>>,
    pub should_register: Arc<Mutex<bool>>,
    pub audio_tx: tokio::sync::broadcast::Sender<Vec<i16>>,
}

/// Helper to build a SipClient and wrap it in a ManagedClient
pub async fn create_managed_client(account: &Account) -> Result<ManagedClient> {
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
        let tls_config = TlsConfig {
            verify_cert: account.tls_verify_cert.unwrap_or(true),
            verify_hostname: account.tls_verify_hostname.unwrap_or(true),
            ca_cert: account.tls_ca_cert.clone(),
        };
        let transport =
            Transport::new_tls(bind_addr, server_addr, &account.domain, &tls_config).await?;
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
        let bound_addr = transport.local_addr()?;
        // Resolve routable local IP directed towards server_addr
        let routed_ip = if bound_addr.ip().is_unspecified() {
            if let Ok(s) = std::net::UdpSocket::bind("0.0.0.0:0") {
                if s.connect(server_addr).is_ok() {
                    s.local_addr().map(|a| a.ip()).unwrap_or(bound_addr.ip())
                } else {
                    bound_addr.ip()
                }
            } else {
                bound_addr.ip()
            }
        } else {
            bound_addr.ip()
        };
        let local_addr = SocketAddr::new(routed_ip, bound_addr.port());
        log::info!(
            "Account '{}' using UDP transport to {} (local {})",
            account.name,
            server_addr,
            local_addr
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
        account.tls_verify_cert,
        account.tls_verify_hostname,
        account.tls_ca_cert.clone(),
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

/// Helper to spawn background watchers (call and registration watchers) for a client.
pub fn spawn_watchers_for_client(name: String, mc: &ManagedClient, shutdown: Arc<Mutex<bool>>) {
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

    let client = mc.client.clone();
    let active = mc.active.clone();
    let should_register = mc.should_register.clone();
    let register_expiry = mc.account.register_expiry.unwrap_or(3600);
    let retry_interval = mc.account.register_retry_interval.unwrap_or(30);

    tokio::spawn(async move {
        registration_watcher(
            name,
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

/// Spawn a periodic RFC 4028 session timer refresher for an active dialog.
pub fn spawn_session_refresher(client: Arc<Mutex<SipClient>>, interval_secs: u64) {
    tokio::spawn(async move {
        // Refresh at interval / 2 (or minimum 10 seconds)
        let sleep_duration = std::time::Duration::from_secs((interval_secs / 2).max(10));
        loop {
            tokio::time::sleep(sleep_duration).await;
            let mut c = client.lock().await;
            if !c.in_call {
                log::debug!("Session refresh loop exiting (call ended)");
                break;
            }
            log::info!("RFC 4028 session timer expired, refreshing active dialog...");
            if let Err(e) = c.refresh_session().await {
                log::error!("RFC 4028 session refresh failed: {}, terminating call", e);
                let _ = c.bye().await;
                break;
            }
        }
    });
}
