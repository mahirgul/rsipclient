use crate::rtp::codec::Codec;
use crate::rtp::receiver::{save_wav, RtpReceiver};
use crate::sip::transfer;
use crate::sip::SipClient;
use crate::ivr::types::{IvrAction, IvrConfig};
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
            let digits = self
                .collect_dtmf(receiver, self.config.timeout_secs, self.config.max_digits)
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
        _receiver: &RtpReceiver,
    ) -> Result<()> {
        let data = std::fs::read(wav_path)?;
        let (_info, samples) = crate::rtp::wav::parse_wav(&data)?;

        let client_guard = client.lock().await;
        let rtp_port = client_guard.rtp_port_start;
        let rate = self.codec.clock_rate();
        drop(client_guard);

        let codec = self.codec;
        let samples_clone = samples.clone();
        tokio::spawn(async move {
            let _ =
                crate::rtp::send_wav_rtp(&samples_clone, rate, remote, 0, rtp_port, codec).await;
        });

        let dur = Duration::from_secs_f64(samples.len() as f64 / rate as f64);
        tokio::time::sleep(dur).await;
        Ok(())
    }

    async fn collect_dtmf(
        &self,
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
            tokio::time::sleep(Duration::from_millis(200)).await;

            let new_digits = receiver.take_dtmf().await;
            all.push_str(&new_digits);

            if all.len() >= max_digits || !new_digits.is_empty() {
                let sub = Instant::now() + Duration::from_secs(2);
                while Instant::now() < sub && all.len() < max_digits {
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
                let msg = transfer::build_refer(
                    &cg.username,
                    &cg.domain,
                    target,
                    &cg.local_addr_str(),
                    &cg.local_tag,
                    &remote_tag,
                    &call_id,
                    cg.next_cseq().await,
                    &cg.new_branch(),
                    &cg.settings,
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
                log::info!("IVR: recording {}s to {}", duration_secs, path);
                receiver.start_recording().await;
                tokio::time::sleep(Duration::from_secs(*duration_secs)).await;
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
                    let msg = transfer::build_hold(
                        &cg.username,
                        &cg.domain,
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
                    let msg = transfer::build_hold(
                        &cg.username,
                        &cg.domain,
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
