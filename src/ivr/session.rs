//! IVR (Interactive Voice Response) session manager.
//!
//! Controls the execution state of an active IVR call, handling incoming DTMF digits,
//! executing configured playback, record, transfer, or menu actions, and stopping on hangup.

use crate::ivr::types::{IvrAction, IvrConfig};
use crate::rtp::codec::Codec;
use crate::rtp::receiver::{save_wav, RtpReceiver};
use crate::sip::transfer;
use crate::sip::SipClient;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Running IVR session
pub struct IvrSession {
    config: IvrConfig,
    codec: Codec,
}

fn load_wav_file(wav_path: &str) -> Result<Vec<u8>> {
    if let Ok(data) = std::fs::read(wav_path) {
        return Ok(data);
    }
    // Try in audio/ directory if not found in current working directory
    let clean = wav_path
        .trim_start_matches("audio/")
        .trim_start_matches("audio\\");
    let audio_dir_path = format!("audio/{}", clean);
    if let Ok(data) = std::fs::read(&audio_dir_path) {
        return Ok(data);
    }
    // Try file basename in current directory if wav_path contained a folder prefix
    if let Some(file_name) = std::path::Path::new(wav_path)
        .file_name()
        .and_then(|f| f.to_str())
    {
        if let Ok(data) = std::fs::read(file_name) {
            return Ok(data);
        }
    }
    anyhow::bail!("Audio file '{}' not found", wav_path);
}

impl IvrSession {
    /// Create a new IVR session
    pub fn new(config: IvrConfig, codec: Codec) -> Self {
        IvrSession { config, codec }
    }

    /// Run the IVR loop on an answered incoming call.
    pub async fn run(
        &self,
        client: &Arc<Mutex<SipClient>>,
        remote: SocketAddr,
        receiver: &RtpReceiver,
    ) -> Result<()> {
        // Play welcome message if configured
        if !self.config.welcome_file.trim().is_empty() {
            if let Err(e) = self
                .play_and_collect(client, &self.config.welcome_file, remote, receiver)
                .await
            {
                log::warn!("IVR welcome announcement warning: {}", e);
            }
        }

        // Menu loop
        loop {
            // Check if call ended
            let in_call = {
                let cg = client.lock().await;
                cg.in_call
            };
            if !in_call {
                break;
            }

            let digits = self
                .collect_dtmf(
                    client,
                    receiver,
                    self.config.timeout_secs,
                    self.config.max_digits,
                )
                .await;

            let first_char = digits.chars().next();
            let action = first_char.and_then(|c| self.config.menu.get(&c).cloned());

            match action {
                Some(IvrAction::Transfer(_)) | Some(IvrAction::Hangup) => {
                    if let Some(act) = action {
                        let should_end =
                            self.execute_action(client, &act, remote, receiver).await?;
                        if should_end {
                            break;
                        }
                    }
                }
                Some(ref act) => {
                    self.execute_action(client, act, remote, receiver).await?;
                }
                None => {
                    if let Some(ref def) = self.config.default_action.clone() {
                        let should_end = self.execute_action(client, def, remote, receiver).await?;
                        if should_end {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    // --- Internals ---

    async fn play_and_collect(
        &self,
        client: &Arc<Mutex<SipClient>>,
        wav_path: &str,
        remote: SocketAddr,
        receiver: &RtpReceiver,
    ) -> Result<()> {
        let data = load_wav_file(wav_path)?;
        let (info, samples) = crate::rtp::wav::parse_wav(&data)?;

        let codec = self.codec;
        let wav_rate = info.sample_rate;
        let samples_clone = samples.clone();
        let socket = receiver.socket().clone();
        tokio::spawn(async move {
            let _ = crate::rtp::send_wav_rtp_on_socket(
                &socket,
                &samples_clone,
                wav_rate,
                remote,
                codec,
            )
            .await;
        });

        let dur = Duration::from_secs_f64(samples.len() as f64 / wav_rate as f64);
        let start_time = Instant::now();
        while Instant::now().duration_since(start_time) < dur {
            let in_call = {
                let cg = client.lock().await;
                cg.in_call
            };
            if !in_call {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Ok(())
    }

    async fn collect_dtmf(
        &self,
        client: &Arc<Mutex<SipClient>>,
        receiver: &RtpReceiver,
        timeout_secs: u64,
        max_digits: usize,
    ) -> String {
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        let mut all = String::new();

        loop {
            if Instant::now() >= deadline {
                break;
            }
            // Check if call ended
            let in_call = {
                let cg = client.lock().await;
                cg.in_call
            };
            if !in_call {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;

            let new_digits = receiver.take_dtmf().await;
            all.push_str(&new_digits);

            if all.len() >= max_digits || !new_digits.is_empty() {
                let sub = Instant::now() + Duration::from_secs(2);
                while Instant::now() < sub && all.len() < max_digits {
                    // Check if call ended inside nested loop too
                    let in_call_nested = {
                        let cg = client.lock().await;
                        cg.in_call
                    };
                    if !in_call_nested {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    let more = receiver.take_dtmf().await;
                    if more.is_empty() {
                        break;
                    }
                    all.push_str(&more);
                }
                break;
            }
        }

        log::info!("IVR DTMF: {:?}", all);
        if !all.is_empty() {
            let cid = {
                let cg = client.lock().await;
                cg.call_id.clone().unwrap_or_default()
            };
            crate::service::logger::record_call_dtmf(&cid, &all);
        }
        all
    }

    async fn execute_action(
        &self,
        client: &Arc<Mutex<SipClient>>,
        action: &IvrAction,
        remote: SocketAddr,
        receiver: &RtpReceiver,
    ) -> Result<bool> {
        match action {
            IvrAction::Transfer(target) => {
                log::info!("IVR: transferring to {}", target);
                let cg = client.lock().await;
                let call_id = cg.call_id.clone().unwrap_or_default();
                let remote_tag = cg.remote_tag.clone().unwrap_or_default();
                let remote_uri = cg.remote_uri.clone().unwrap_or_default();
                let msg = transfer::build_refer(
                    &cg.username,
                    &cg.domain,
                    &remote_uri,
                    target,
                    &cg.local_addr_str(),
                    &cg.local_tag,
                    &remote_tag,
                    &call_id,
                    cg.next_cseq().await,
                    &cg.new_branch(),
                    &cg.settings,
                    cg.transport.via_str(),
                );
                drop(cg);
                self.send_sip(client, &msg).await?;
                Ok(true)
            }

            IvrAction::Playback(path) => {
                log::info!("IVR: playing {}", path);
                self.play_and_collect(client, path, remote, receiver)
                    .await?;
                Ok(false)
            }

            IvrAction::Record {
                path,
                duration_secs,
            } => {
                log::info!("IVR: recording up to {}s to {}", duration_secs, path);
                receiver.start_recording().await;
                let start_time = Instant::now();
                let max_dur = Duration::from_secs(*duration_secs);
                while Instant::now().duration_since(start_time) < max_dur {
                    let in_call = {
                        let cg = client.lock().await;
                        cg.in_call
                    };
                    if !in_call {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                let samples = receiver.stop_recording().await;
                save_wav(&samples, self.codec.clock_rate(), path)?;
                log::info!("IVR: saved {} samples to {}", samples.len(), path);
                Ok(false)
            }

            IvrAction::Hold => {
                log::info!("IVR: holding call");
                {
                    let cg = client.lock().await;
                    let call_id = cg.call_id.clone().unwrap_or_default();
                    let remote_tag = cg.remote_tag.clone().unwrap_or_default();
                    let remote_uri = cg.remote_uri.clone().unwrap_or_default();
                    let msg = transfer::build_hold(
                        &cg.username,
                        &cg.domain,
                        &remote_uri,
                        &cg.local_addr.ip().to_string(),
                        &cg.local_addr_str(),
                        &cg.local_tag,
                        &remote_tag,
                        &call_id,
                        cg.next_cseq().await,
                        &cg.new_branch(),
                        cg.rtp_port_start,
                        &cg.settings,
                        false,
                        self.codec.to_config_str(),
                        cg.transport.via_str(),
                    );
                    drop(cg);
                    self.send_sip(client, &msg).await?;
                }

                log::info!("IVR: waiting for DTMF to resume...");
                loop {
                    let d = receiver.take_dtmf().await;
                    if !d.is_empty() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(300)).await;
                }

                {
                    let cg = client.lock().await;
                    let call_id = cg.call_id.clone().unwrap_or_default();
                    let remote_tag = cg.remote_tag.clone().unwrap_or_default();
                    let remote_uri = cg.remote_uri.clone().unwrap_or_default();
                    let msg = transfer::build_hold(
                        &cg.username,
                        &cg.domain,
                        &remote_uri,
                        &cg.local_addr.ip().to_string(),
                        &cg.local_addr_str(),
                        &cg.local_tag,
                        &remote_tag,
                        &call_id,
                        cg.next_cseq().await,
                        &cg.new_branch(),
                        cg.rtp_port_start,
                        &cg.settings,
                        true,
                        self.codec.to_config_str(),
                        cg.transport.via_str(),
                    );
                    drop(cg);
                    self.send_sip(client, &msg).await?;
                }
                Ok(false)
            }

            IvrAction::Hangup => {
                log::info!("IVR: hanging up");
                let mut cg = client.lock().await;
                let _ = cg.bye().await;
                drop(cg);
                Ok(true)
            }

            IvrAction::Webhook(url) => {
                log::info!("IVR: executing Webhook action target {}", url);
                let mut ctx = std::collections::HashMap::new();
                let (account, caller, call_id) = {
                    let cg = client.lock().await;
                    (
                        cg.username.clone(),
                        cg.remote_uri.clone().unwrap_or_default(),
                        cg.call_id.clone().unwrap_or_default(),
                    )
                };
                ctx.insert("account".to_string(), account);
                ctx.insert("caller".to_string(), caller);
                ctx.insert("call_id".to_string(), call_id);

                let client_http = reqwest::Client::new();
                if let Ok(resp) = client_http.post(url).json(&ctx).send().await {
                    if let Ok(plugin_res) = resp.json::<crate::plugins::PluginActionResult>().await
                    {
                        let inner_action = match plugin_res {
                            crate::plugins::PluginActionResult::Transfer { target } => {
                                IvrAction::Transfer(target)
                            }
                            crate::plugins::PluginActionResult::Playback { target } => {
                                IvrAction::Playback(target)
                            }
                            crate::plugins::PluginActionResult::Record { target, duration } => {
                                IvrAction::Record {
                                    path: target,
                                    duration_secs: duration.unwrap_or(10),
                                }
                            }
                            crate::plugins::PluginActionResult::Hold => IvrAction::Hold,
                            crate::plugins::PluginActionResult::Hangup => IvrAction::Hangup,
                            crate::plugins::PluginActionResult::None => return Ok(false),
                        };
                        return Box::pin(self.execute_action(
                            client,
                            &inner_action,
                            remote,
                            receiver,
                        ))
                        .await;
                    }
                }
                Ok(false)
            }

            IvrAction::Script(script_path) => {
                log::info!("IVR: executing Script action target {}", script_path);
                let mut ctx = std::collections::HashMap::new();
                let (account, caller, call_id) = {
                    let cg = client.lock().await;
                    (
                        cg.username.clone(),
                        cg.remote_uri.clone().unwrap_or_default(),
                        cg.call_id.clone().unwrap_or_default(),
                    )
                };
                ctx.insert("account".to_string(), account);
                ctx.insert("caller".to_string(), caller);
                ctx.insert("call_id".to_string(), call_id);

                let path = std::path::Path::new(script_path);
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                let res = match ext {
                    "rhai" => crate::plugins::rhai_runner::RhaiRunner::new().execute_script(
                        path,
                        None,
                        Some(ctx),
                    ),
                    "lua" => crate::plugins::lua_runner::LuaRunner::new().execute_script(
                        path,
                        None,
                        Some(ctx),
                    ),
                    _ => Err(format!("Unknown script extension '{}'", ext)),
                };

                if let Ok(plugin_res) = res {
                    let inner_action = match plugin_res {
                        crate::plugins::PluginActionResult::Transfer { target } => {
                            IvrAction::Transfer(target)
                        }
                        crate::plugins::PluginActionResult::Playback { target } => {
                            IvrAction::Playback(target)
                        }
                        crate::plugins::PluginActionResult::Record { target, duration } => {
                            IvrAction::Record {
                                path: target,
                                duration_secs: duration.unwrap_or(10),
                            }
                        }
                        crate::plugins::PluginActionResult::Hold => IvrAction::Hold,
                        crate::plugins::PluginActionResult::Hangup => IvrAction::Hangup,
                        crate::plugins::PluginActionResult::None => return Ok(false),
                    };
                    return Box::pin(self.execute_action(client, &inner_action, remote, receiver))
                        .await;
                }
                Ok(false)
            }
        }
    }

    async fn send_sip(&self, client: &Arc<Mutex<SipClient>>, msg: &str) -> Result<()> {
        let c = client.lock().await;
        c.send(msg).await?;
        Ok(())
    }
}
