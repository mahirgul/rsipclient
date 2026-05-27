//! RTP receiver — listen for incoming RTP, detect DTMF (RFC 2833),
//! and optionally record audio to WAV.

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

/// Detected DTMF event
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct DtmfEvent {
    /// DTMF digit: '0'-'9', '*', '#', 'A'-'D'
    pub digit: char,
    /// Event duration in RTP timestamp units
    pub duration: u16,
    /// Whether this is the end of the event
    pub end: bool,
}

/// RTP receiver state
pub struct RtpReceiver {
    socket: Arc<UdpSocket>,
    /// Collected DTMF digits (RFC 2833 telephone-event, PT=101)
    dtmf_buffer: Arc<Mutex<String>>,
    /// Pending DTMF events
    dtmf_events: Arc<Mutex<Vec<DtmfEvent>>>,
    /// Recorded audio (linear 16-bit PCM)
    recording: Arc<Mutex<Vec<i16>>>,
    /// Whether recording is active
    recording_active: Arc<Mutex<bool>>,
    /// Last sequence number seen
    last_seq: Arc<Mutex<Option<u16>>>,
}

impl RtpReceiver {
    /// Bind to the given port and start listening.
    pub async fn bind(local_port: u16) -> Result<Self> {
        let addr: SocketAddr = format!("0.0.0.0:{}", local_port).parse()?;
        let socket = UdpSocket::bind(addr).await?;
        Ok(RtpReceiver {
            socket: Arc::new(socket),
            dtmf_buffer: Arc::new(Mutex::new(String::new())),
            dtmf_events: Arc::new(Mutex::new(Vec::new())),
            recording: Arc::new(Mutex::new(Vec::new())),
            recording_active: Arc::new(Mutex::new(false)),
            last_seq: Arc::new(Mutex::new(None)),
        })
    }

    /// Start background receive loop (non-blocking).
    /// Spawns a task that continuously reads RTP packets and processes them.
    pub fn start(&self) {
        let socket = self.socket.clone();
        let dtmf_buf = self.dtmf_buffer.clone();
        let dtmf_events = self.dtmf_events.clone();
        let recording = self.recording.clone();
        let recording_active = self.recording_active.clone();
        let last_seq = self.last_seq.clone();

        tokio::spawn(async move {
            let mut buf = [0u8; 2048];
            loop {
                match socket.recv_from(&mut buf).await {
                    Ok((n, _src)) => {
                        if n < 12 {
                            continue;
                        }
                        let pt = buf[1] & 0x7F;

                        if pt == 101 {
                            // RFC 2833 telephone-event
                            if let Some(dtmf) = parse_dtmf(&buf[12..n]) {
                                let mut digits = dtmf_buf.lock().await;
                                let mut events = dtmf_events.lock().await;
                                if dtmf.end
                                    && !dtmf.digit.is_whitespace()
                                    && !digits.ends_with(dtmf.digit)
                                {
                                    digits.push(dtmf.digit);
                                }
                                events.push(dtmf);
                            }
                        } else {
                            // Audio packet — record if active
                            let active = *recording_active.lock().await;
                            if active {
                                let payload = &buf[12..n];
                                let samples: Vec<i16> = payload
                                    .chunks_exact(2)
                                    .map(|c| i16::from_be_bytes([c[0], c[1]]))
                                    .collect();
                                recording.lock().await.extend(&samples);
                            }

                            // Track sequence
                            let seq = u16::from_be_bytes([buf[2], buf[3]]);
                            *last_seq.lock().await = Some(seq);
                        }
                    }
                    Err(e) => {
                        log::error!("RTP receive error: {}", e);
                        break;
                    }
                }
            }
        });
    }

    /// Get accumulated DTMF digits and clear buffer
    pub async fn take_dtmf(&self) -> String {
        let mut buf = self.dtmf_buffer.lock().await;
        let digits = buf.clone();
        buf.clear();
        digits
    }

    /// Get pending DTMF events
    #[allow(dead_code)]
    pub async fn take_dtmf_events(&self) -> Vec<DtmfEvent> {
        let mut events = self.dtmf_events.lock().await;
        std::mem::take(&mut *events)
    }

    /// Start recording incoming audio
    pub async fn start_recording(&self) {
        *self.recording_active.lock().await = true;
        self.recording.lock().await.clear();
    }

    /// Stop recording and return captured samples
    pub async fn stop_recording(&self) -> Vec<i16> {
        *self.recording_active.lock().await = false;
        self.recording.lock().await.clone()
    }
}

/// Parse an RFC 2833 telephone-event RTP payload
fn parse_dtmf(payload: &[u8]) -> Option<DtmfEvent> {
    if payload.len() < 4 {
        return None;
    }

    let event = payload[0];
    let e_bit = (payload[1] & 0x80) != 0;
    let duration = u16::from_be_bytes([payload[2], payload[3]]);

    let digit = match event {
        0..=9 => char::from_digit(event as u32, 10).unwrap(),
        10 => '*',
        11 => '#',
        12 => 'A',
        13 => 'B',
        14 => 'C',
        15 => 'D',
        16 => ' ', // flash
        _ => return None,
    };

    Some(DtmfEvent {
        digit,
        duration,
        end: e_bit,
    })
}

/// Save linear 16-bit PCM samples as a WAV file
pub fn save_wav(samples: &[i16], sample_rate: u32, path: &str) -> Result<()> {
    use std::io::Write;
    let file = std::fs::File::create(path)?;
    let mut writer = std::io::BufWriter::new(file);

    let data_len = (samples.len() * 2) as u32; // 16-bit = 2 bytes each
    let riff_size: u32 = 36 + data_len;

    // RIFF header
    writer.write_all(b"RIFF")?;
    writer.write_all(&riff_size.to_le_bytes())?;
    writer.write_all(b"WAVE")?;

    // fmt chunk
    writer.write_all(b"fmt ")?;
    writer.write_all(&16u32.to_le_bytes())?; // chunk size
    writer.write_all(&1u16.to_le_bytes())?; // PCM
    writer.write_all(&1u16.to_le_bytes())?; // mono
    writer.write_all(&sample_rate.to_le_bytes())?;
    writer.write_all(&(sample_rate * 2).to_le_bytes())?; // byte rate
    writer.write_all(&2u16.to_le_bytes())?; // block align
    writer.write_all(&16u16.to_le_bytes())?; // bits per sample

    // data chunk
    writer.write_all(b"data")?;
    writer.write_all(&data_len.to_le_bytes())?;
    for &s in samples {
        writer.write_all(&s.to_le_bytes())?;
    }
    writer.flush()?;
    Ok(())
}
