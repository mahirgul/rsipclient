//! Background watcher for SIP registration and NAT keepalive.
//!
//! Monitors connection state, handles periodic re-registration, and transmits keepalive frames.

use crate::sip::SipClient;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Background task: monitor registration state of a client and refresh or retry accordingly.
pub async fn registration_watcher(
    account_name: String,
    client: Arc<Mutex<SipClient>>,
    active: Arc<Mutex<bool>>,
    should_register: Arc<Mutex<bool>>,
    register_expiry: u32,
    retry_interval: u32,
    shutdown: Arc<Mutex<bool>>,
) {
    let mut last_register_time: Option<std::time::Instant> = None;
    let mut last_keepalive_time: Option<std::time::Instant> = None;
    let mut is_currently_registered = false;

    loop {
        if *shutdown.lock().await || !*active.lock().await {
            // Only unregister if we actually wanted to be registered
            let want_register = *should_register.lock().await;
            if want_register {
                let is_reg = {
                    let c = client.lock().await;
                    let val = *c.registered.lock().await;
                    val
                };
                if is_reg {
                    log::info!("[{}] Unregistering on shutdown/deactivate...", account_name);
                    let _ = client.lock().await.unregister().await;
                }
            }
            break;
        }

        let want_register = *should_register.lock().await;

        if want_register {
            let now = std::time::Instant::now();
            let need_retry_or_refresh = match last_register_time {
                None => true, // never tried
                Some(last_time) => {
                    if is_currently_registered {
                        // Refresh registration at half of expiry time
                        let refresh_duration =
                            std::time::Duration::from_secs((register_expiry / 2).max(10) as u64);
                        now.duration_since(last_time) >= refresh_duration
                    } else {
                        // Retry registration after retry_interval
                        let retry_duration =
                            std::time::Duration::from_secs(retry_interval.max(5) as u64);
                        now.duration_since(last_time) >= retry_duration
                    }
                }
            };

            if need_retry_or_refresh {
                log::info!("[{}] Attempting registration...", account_name);
                let reg_res = {
                    let c = client.lock().await;
                    c.register().await
                };

                last_register_time = Some(std::time::Instant::now());
                match reg_res {
                    Ok(true) => {
                        log::info!("[{}] Registration successful", account_name);
                        is_currently_registered = true;
                    }
                    Ok(false) => {
                        log::warn!("[{}] Registration failed, will retry", account_name);
                        is_currently_registered = false;
                        // Force register flag in client to false
                        let c = client.lock().await;
                        *c.registered.lock().await = false;
                    }
                    Err(e) => {
                        log::error!("[{}] Registration error: {}, will retry", account_name, e);
                        is_currently_registered = false;
                        // Force register flag in client to false
                        let c = client.lock().await;
                        *c.registered.lock().await = false;
                    }
                }
            }
        } else {
            // If they don't want to be registered, but are registered, unregister them
            let is_reg = {
                let c = client.lock().await;
                let reg_val = *c.registered.lock().await;
                reg_val
            };
            if is_reg {
                log::info!("[{}] Unregistering as requested...", account_name);
                let reg_res = {
                    let c = client.lock().await;
                    c.unregister().await
                };
                if let Err(e) = reg_res {
                    log::error!("[{}] Unregistration error: {}", account_name, e);
                }
                is_currently_registered = false;
                last_register_time = None;
            }
        }

        // NAT Keep-alive logic: send double CRLF every 20 seconds when registered.
        // If a keep-alive fails (e.g. socket disconnected), we mark the account as unregistered
        // and force an immediate re-registration retry instead of waiting for half-expiry.
        if is_currently_registered {
            let now = std::time::Instant::now();
            let need_keepalive = match last_keepalive_time {
                None => true,
                Some(t) => now.duration_since(t) >= std::time::Duration::from_secs(20),
            };
            if need_keepalive {
                let c = client.lock().await;
                match c.send_keepalive().await {
                    Ok(_) => {
                        last_keepalive_time = Some(now);
                    }
                    Err(e) => {
                        log::warn!(
                            "[{}] NAT keep-alive failed: {}. Connection might be dead. Forcing re-registration...",
                            account_name,
                            e
                        );
                        is_currently_registered = false;
                        last_register_time = None;
                        last_keepalive_time = None;
                        *c.registered.lock().await = false;
                    }
                }
            }
        } else {
            last_keepalive_time = None;
        }

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
