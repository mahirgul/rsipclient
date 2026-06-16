//! Web Dashboard API handlers for status and logging.
//!
//! Provides endpoints to fetch active call statuses, system resource utilization, and memory logs.

use super::super::web_server::{verify_token, AccountStatus, AppState, StatusResponse};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use sysinfo::System;

/// Get call status, registrations, and process diagnostics
pub async fn get_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    let cls = {
        let guard = state.clients.lock().await;
        guard.clone()
    };

    let mut total_accounts = 0;
    let mut registered_accounts = 0;
    let mut active_calls = 0;
    let mut accounts = vec![];

    for (name, mc) in cls.iter() {
        total_accounts += 1;
        let client_lock = mc.client.lock().await;
        let is_registered = *client_lock.registered.lock().await;
        let is_in_call = client_lock.in_call;

        if is_registered {
            registered_accounts += 1;
        }
        if is_in_call {
            active_calls += 1;
        }

        let codec_str = mc
            .account
            .codec
            .clone()
            .unwrap_or_else(|| "pcmu".to_string());
        let codec_rate = mc.codec.clock_rate();

        let is_held = client_lock.held;

        accounts.push(AccountStatus {
            name: name.clone(),
            username: client_lock.username.clone(),
            domain: client_lock.domain.clone(),
            server: client_lock.server_addr.to_string(),
            sip_port: client_lock.local_addr.port(),
            registered: is_registered,
            in_call: is_in_call,
            held: is_held,
            call_id: client_lock.call_id.clone(),
            codec: codec_str,
            codec_rate,
        });
    }

    // Process system and resource info
    let pid = sysinfo::get_current_pid().ok();
    let mut memory_bytes = 0;
    let mut cpu_percent = 0.0;

    if let Some(pid) = pid {
        let mut sys = state.sys.lock().await;
        sys.refresh_process(pid);
        if let Some(proc) = sys.process(pid) {
            memory_bytes = proc.memory();

            // Normalize CPU usage against logical CPU cores to show a value between 0% and 100%
            let cpu_cores = sys.cpus().len();
            let raw_cpu = proc.cpu_usage();
            cpu_percent = if cpu_cores > 0 {
                raw_cpu / cpu_cores as f32
            } else {
                raw_cpu
            };
        }
    }

    let uptime_secs = state.start_time.elapsed().as_secs();
    let os_name = format!(
        "{} {}",
        System::name().unwrap_or_else(|| "Unknown OS".to_string()),
        System::os_version().unwrap_or_default()
    );

    Ok(Json(StatusResponse {
        uptime_secs,
        memory_bytes,
        cpu_percent,
        os_name,
        total_accounts,
        registered_accounts,
        active_calls,
        accounts,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        config_path: state.config_path.clone(),
    }))
}

/// Fetch recent logs captured in memory
pub async fn get_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    verify_token(&headers, &state)?;

    let logs = super::super::logger::get_recent_logs();
    Ok(Json(logs))
}
