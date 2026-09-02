//! RTP receiver — listen for incoming RTP, detect DTMF (RFC 2833),
//! and optionally record audio to WAV.

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

/// Cap on buffered recording samples (30 minutes at 8 kHz).
///
/// The receive loop appends every decoded packet, and a caller that never stops
/// recording — or a peer that keeps sending — would otherwise grow this without
/// bound.
const MAX_RECORDING_SAMPLES: usize = 8_000 * 60 * 30;

/// Cap on buffered DTMF collected from the peer.
///
/// Both buffers are drained by whoever asked for the digits — the IVR session,
/// or nobody at all when the account auto-answers without a menu. A peer that
/// keeps sending telephone-events would otherwise grow them for the whole call.
/// Far more than any menu reads, and old entries give way to new ones.
const MAX_DTMF_BUFFERED: usize = 512;

/// The RTP header fields the receive loop acts on.
pub(crate) struct RtpPacket<'a> {
    pub payload_type: u8,
    pub sequence: u16,
    pub payload: &'a [u8],
}

/// Parse an RTP packet header (RFC 3550 section 5.1).
///
/// The payload does not start at a fixed offset: CSRC entries and a header
/// extension push it back, and padding trims the end. Assuming a bare 12-byte
/// header hands the codec parts of the header as audio whenever a peer uses
/// either feature.
pub(crate) fn parse_rtp(packet: &[u8]) -> Option<RtpPacket<'_>> {
    if packet.len() < 12 || packet[0] >> 6 != 2 {
        return None;
    }

    let has_padding = packet[0] & 0x20 != 0;
    let has_extension = packet[0] & 0x10 != 0;
    let csrc_count = (packet[0] & 0x0F) as usize;

    let mut start = 12 + 4 * csrc_count;
    if packet.len() < start {
        return None;
    }

    if has_extension {
        if packet.len() < start + 4 {
            return None;
        }
        let words = u16::from_be_bytes([packet[start + 2], packet[start + 3]]) as usize;
        start += 4 + 4 * words;
        if packet.len() < start {
            return None;
        }
    }

    let mut end = packet.len();
    if has_padding {
        let pad = packet[end - 1] as usize;
        if pad == 0 || pad > end - start {
            return None;
        }
        end -= pad;
    }

    Some(RtpPacket {
        payload_type: packet[1] & 0x7F,
        sequence: u16::from_be_bytes([packet[2], packet[3]]),
        payload: &packet[start..end],
    })
}

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
#[derive(Clone)]
pub struct RtpReceiver {
    socket: Arc<UdpSocket>,
    /// Signal to stop the background receive loop
    stop_flag: Arc<AtomicBool>,
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
            stop_flag: Arc::new(AtomicBool::new(false)),
            dtmf_buffer: Arc::new(Mutex::new(String::new())),
            dtmf_events: Arc::new(Mutex::new(Vec::new())),
            recording: Arc::new(Mutex::new(Vec::new())),
            recording_active: Arc::new(Mutex::new(false)),
            last_seq: Arc::new(Mutex::new(None)),
        })
    }

    /// Try to bind to any port in the range start..=end.
    /// Returns the receiver and the bound port.
    pub async fn bind_range(start: u16, end: u16) -> Result<(Self, u16)> {
        let mut last_err = None;
        for port in start..=end {
            match Self::bind(port).await {
                Ok(receiver) => return Ok((receiver, port)),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Invalid port range: {}-{}", start, end)))
    }

    /// Signal the background receive loop to stop (idempotent, thread-safe)
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }

    /// Get the underlying UDP socket (cloned Arc)
    pub fn socket(&self) -> Arc<UdpSocket> {
        self.socket.clone()
    }

    /// Start background receive loop (non-blocking).
    /// Spawns a task that continuously reads RTP packets and processes them.
    pub fn start(
        &self,
        codec: crate::rtp::codec::Codec,
        audio_tx: Option<tokio::sync::broadcast::Sender<Vec<i16>>>,
    ) {
        let socket = self.socket.clone();
        let stop_flag = self.stop_flag.clone();
        let dtmf_buf = self.dtmf_buffer.clone();
        let dtmf_events = self.dtmf_events.clone();
        let recording = self.recording.clone();
        let recording_active = self.recording_active.clone();
        let last_seq = self.last_seq.clone();

        tokio::spawn(async move {
            let mut buf = [0u8; 2048];
            // Symmetric-RTP latch: the first peer heard owns this session. Without
            // it anything that can reach the port injects audio and DTMF into a
            // live call. Learned rather than taken from the SDP, because NAT'd
            // peers routinely send from a different port than they advertise.
            let mut peer: Option<SocketAddr> = None;
            let mut recording_full = false;
            loop {
                // Check stop signal before each recv
                if stop_flag.load(Ordering::Relaxed) {
                    log::debug!("RTP receive loop stopped via stop signal");
                    break;
                }

                // Wrap recv in timeout so stop_flag is checked regularly
                let recv_result = tokio::time::timeout(
                    std::time::Duration::from_secs(1),
                    socket.recv_from(&mut buf),
                )
                .await;

                match recv_result {
                    Ok(Ok((n, src))) => {
                        match peer {
                            None => {
                                log::debug!("RTP stream latched to {}", src);
                                peer = Some(src);
                            }
                            Some(known) if known != src => {
                                log::debug!("Ignoring RTP packet from unexpected source {}", src);
                                continue;
                            }
                            Some(_) => {}
                        }

                        let rtp = match parse_rtp(&buf[..n]) {
                            Some(rtp) => rtp,
                            None => continue,
                        };

                        if rtp.payload_type == 101 {
                            // RFC 2833 telephone-event
                            if let Some(dtmf) = parse_dtmf(rtp.payload) {
                                let mut digits = dtmf_buf.lock().await;
                                let mut events = dtmf_events.lock().await;
                                if dtmf.end
                                    && !dtmf.digit.is_whitespace()
                                    && !digits.ends_with(dtmf.digit)
                                {
                                    if digits.chars().count() >= MAX_DTMF_BUFFERED {
                                        digits.remove(0);
                                    }
                                    digits.push(dtmf.digit);
                                }
                                if events.len() >= MAX_DTMF_BUFFERED {
                                    events.remove(0);
                                }
                                events.push(dtmf);
                            }
                        } else {
                            // Audio packet — decode first
                            if let Ok(samples) = codec.decode(rtp.payload) {
                                let active = *recording_active.lock().await;
                                if active {
                                    let mut rec = recording.lock().await;
                                    let room = MAX_RECORDING_SAMPLES.saturating_sub(rec.len());
                                    if room >= samples.len() {
                                        rec.extend(&samples);
                                    } else {
                                        rec.extend(&samples[..room]);
                                        if !recording_full {
                                            recording_full = true;
                                            log::warn!(
                                                "Recording buffer full ({} samples); \
                                                 dropping further audio until recording is stopped",
                                                MAX_RECORDING_SAMPLES
                                            );
                                        }
                                    }
                                }
                                if let Some(ref tx) = audio_tx {
                                    let _ = tx.send(samples);
                                }
                            }

                            *last_seq.lock().await = Some(rtp.sequence);
                        }
                    }
                    Ok(Err(e)) => {
                        log::error!("RTP receive error: {}", e);
                        break;
                    }
                    Err(_elapsed) => {
                        // Timeout — just loop back to check stop_flag
                        continue;
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

    /// Send raw PCM samples as RTP to the target address
    pub async fn send_audio_samples(
        &self,
        samples: &[i16],
        target: SocketAddr,
        codec: crate::rtp::codec::Codec,
        seq: &mut u16,
        timestamp: &mut u32,
    ) -> Result<()> {
        let payload = codec.encode(samples)?;
        let ssrc: u32 = rand::random();

        let mut packet = Vec::with_capacity(12 + payload.len());
        packet.push(0x80); // V=2, P=0, X=0, CC=0
        packet.push(codec.payload_type());
        packet.extend_from_slice(&seq.to_be_bytes());
        packet.extend_from_slice(&timestamp.to_be_bytes());
        packet.extend_from_slice(&ssrc.to_be_bytes());
        packet.extend_from_slice(&payload);

        self.socket.send_to(&packet, target).await?;
        *seq = seq.wrapping_add(1);
        *timestamp = timestamp.wrapping_add(samples.len() as u32);
        Ok(())
    }

    /// Send a single DTMF digit (RFC 2833 telephone-event, PT=101)
    pub async fn send_dtmf_digit(
        &self,
        digit: char,
        target: SocketAddr,
        seq: &mut u16,
        timestamp: &mut u32,
    ) -> Result<()> {
        let event = match digit {
            '0'..='9' => digit as u8 - b'0',
            '*' => 10,
            '#' => 11,
            'A'..='D' => digit as u8 - b'A' + 12,
            'a'..='d' => digit as u8 - b'a' + 12,
            _ => {
                log::warn!("Invalid DTMF digit: '{}'", digit);
                return Ok(());
            }
        };

        let ssrc: u32 = rand::random();
        let event_timestamp = *timestamp;

        // Send 3 intermediate packets (duration = 160, 320, 480)
        for step in 1..=3 {
            let duration = (step * 160) as u16;
            let mut payload = vec![0u8; 4];
            payload[0] = event;
            payload[1] = 0x0A; // E=0, R=0, Volume=10
            payload[2] = (duration >> 8) as u8;
            payload[3] = (duration & 0xFF) as u8;

            let mut packet = Vec::with_capacity(12 + payload.len());
            packet.push(0x80); // V=2, P=0, X=0, CC=0
            packet.push(101); // Payload type for telephone-event
            packet.extend_from_slice(&seq.to_be_bytes());
            packet.extend_from_slice(&event_timestamp.to_be_bytes());
            packet.extend_from_slice(&ssrc.to_be_bytes());
            packet.extend_from_slice(&payload);

            let _ = self.socket.send_to(&packet, target).await;
            *seq = seq.wrapping_add(1);

            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        // Send 3 end packets (E=1, same duration)
        let final_duration = 480u16;
        for _ in 1..=3 {
            let mut payload = vec![0u8; 4];
            payload[0] = event;
            payload[1] = 0x8A; // E=1, R=0, Volume=10
            payload[2] = (final_duration >> 8) as u8;
            payload[3] = (final_duration & 0xFF) as u8;

            let mut packet = Vec::with_capacity(12 + payload.len());
            packet.push(0x80); // V=2, P=0, X=0, CC=0
            packet.push(101); // Payload type for telephone-event
            packet.extend_from_slice(&seq.to_be_bytes());
            packet.extend_from_slice(&event_timestamp.to_be_bytes());
            packet.extend_from_slice(&ssrc.to_be_bytes());
            packet.extend_from_slice(&payload);

            let _ = self.socket.send_to(&packet, target).await;
            *seq = seq.wrapping_add(1);
        }

        // Increment the main timestamp by the event duration (plus standard gap)
        *timestamp = timestamp.wrapping_add(800);

        // Wait a short gap between digits
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn header(first_byte: u8, payload_type: u8, seq: u16) -> Vec<u8> {
        let mut p = vec![first_byte, payload_type];
        p.extend_from_slice(&seq.to_be_bytes());
        p.extend_from_slice(&0u32.to_be_bytes()); // timestamp
        p.extend_from_slice(&0u32.to_be_bytes()); // ssrc
        p
    }

    #[test]
    fn parses_a_plain_packet() {
        let mut p = header(0x80, 0, 42);
        p.extend_from_slice(&[1, 2, 3, 4]);
        let rtp = parse_rtp(&p).expect("should parse");
        assert_eq!(rtp.payload_type, 0);
        assert_eq!(rtp.sequence, 42);
        assert_eq!(rtp.payload, &[1, 2, 3, 4]);
    }

    /// CSRC entries push the payload back by 4 bytes each; a fixed 12-byte
    /// offset would hand the codec part of the header as audio.
    #[test]
    fn skips_csrc_entries() {
        let mut p = header(0x82, 8, 1); // CC = 2
        p.extend_from_slice(&[0xAA; 8]);
        p.extend_from_slice(&[1, 2, 3, 4]);
        let rtp = parse_rtp(&p).expect("should parse");
        assert_eq!(rtp.payload, &[1, 2, 3, 4]);
    }

    #[test]
    fn skips_header_extension() {
        let mut p = header(0x90, 8, 1); // X = 1
        p.extend_from_slice(&[0xBE, 0xDE, 0x00, 0x01]); // profile + 1 word
        p.extend_from_slice(&[0xCC; 4]);
        p.extend_from_slice(&[1, 2, 3, 4]);
        let rtp = parse_rtp(&p).expect("should parse");
        assert_eq!(rtp.payload, &[1, 2, 3, 4]);
    }

    #[test]
    fn trims_padding() {
        let mut p = header(0xA0, 8, 1); // P = 1
        p.extend_from_slice(&[1, 2, 3, 4]);
        p.extend_from_slice(&[0, 0, 3]); // 3 padding bytes, last one is the count
        let rtp = parse_rtp(&p).expect("should parse");
        assert_eq!(rtp.payload, &[1, 2, 3, 4]);
    }

    #[test]
    fn rejects_malformed_packets() {
        assert!(parse_rtp(&[]).is_none());
        assert!(parse_rtp(&[0x80; 11]).is_none(), "short header");
        assert!(parse_rtp(&header(0x00, 0, 1)).is_none(), "wrong version");
        assert!(
            parse_rtp(&header(0x8F, 0, 1)).is_none(),
            "CC claims more CSRCs than the packet holds"
        );
        assert!(
            parse_rtp(&header(0x90, 0, 1)).is_none(),
            "X set but no extension header"
        );

        let mut over_padded = header(0xA0, 8, 1);
        over_padded.extend_from_slice(&[1, 2, 9]); // claims 9 bytes of padding
        assert!(parse_rtp(&over_padded).is_none());

        let mut zero_pad = header(0xA0, 8, 1);
        zero_pad.extend_from_slice(&[1, 2, 0]);
        assert!(parse_rtp(&zero_pad).is_none());
    }

    #[test]
    fn accepts_an_empty_payload() {
        let packet = header(0x80, 8, 7);
        let rtp = parse_rtp(&packet).expect("should parse");
        assert!(rtp.payload.is_empty());
        assert_eq!(rtp.sequence, 7);
    }

    #[test]
    fn parse_dtmf_reads_event_and_end_bit() {
        let event = parse_dtmf(&[5, 0x8A, 0x01, 0xE0]).expect("should parse");
        assert_eq!(event.digit, '5');
        assert!(event.end);
        assert_eq!(event.duration, 480);

        assert!(parse_dtmf(&[1, 0, 0]).is_none(), "short payload");
        assert!(parse_dtmf(&[99, 0, 0, 0]).is_none(), "unknown event");
    }
}
