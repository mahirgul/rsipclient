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
        // Play welcome message
        self.play_and_collect(client, &self.config.welcome_file, remote, receiver)
            .await?;

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
        let data = std::fs::read(wav_path)?;
        let (_info, samples) = crate::rtp::wav::parse_wav(&data)?;

        let client_guard = client.lock().await;
        let rate = self.codec.clock_rate();
        drop(client_guard);

        let codec = self.codec;
        let samples_clone = samples.clone();
        let socket = receiver.socket().clone();
        tokio::spawn(async move {
            let _ =
                crate::rtp::send_wav_rtp_on_socket(&socket, &samples_clone, rate, remote, codec)
                    .await;
        });

        let dur = Duration::from_secs_f64(samples.len() as f64 / rate as f64);
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
        }
    }

    async fn send_sip(&self, client: &Arc<Mutex<SipClient>>, msg: &str) -> Result<()> {
        let c = client.lock().await;
        c.send(msg).await?;
        Ok(())
    }
}
