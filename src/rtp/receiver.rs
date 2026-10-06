//! RTP receiver — listen for incoming RTP, detect DTMF (RFC 2833),
//! and optionally record audio to WAV.

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

use crate::rtp::codec::{AudioDecoder, AudioEncoder, Codec};

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
pub struct RtpPacket<'a> {
    pub payload_type: u8,
    pub sequence: u16,
    pub timestamp: u32,
    pub payload: &'a [u8],
}

/// Parse an RTP packet header (RFC 3550 section 5.1).
///
/// The payload does not start at a fixed offset: CSRC entries and a header
/// extension push it back, and padding trims the end. Assuming a bare 12-byte
/// header hands the codec parts of the header as audio whenever a peer uses
/// either feature.
pub fn parse_rtp(packet: &[u8]) -> Option<RtpPacket<'_>> {
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
        timestamp: u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]),
        payload: &packet[start..end],
    })
}

/// RTP payload type of RFC 2833 / RFC 4733 telephone-events.
const TELEPHONE_EVENT_PT: u8 = 101;
/// RTP payload type of RFC 3389 comfort noise.
const COMFORT_NOISE_PT: u8 = 13;
/// Packetization interval used for everything we send.
const PTIME_MS: u32 = 20;

/// Whether an incoming payload type carries audio for `codec`.
///
/// G.711 types are static, so anything else (comfort noise, stray streams)
/// would decode to noise. Opus is dynamic and the peer may have picked any
/// number in the dynamic range.
fn is_audio_payload(pt: u8, codec: Codec) -> bool {
    match codec {
        Codec::Pcmu | Codec::Pcma => pt == codec.payload_type(),
        Codec::Opus => (96..=127).contains(&pt) && pt != TELEPHONE_EVENT_PT,
    }
}

/// State of the single outgoing RTP stream of a call.
///
/// Audio, RFC 2833 events and in-band DTMF all go out on one socket, so they
/// must share one SSRC with continuous sequence numbers and timestamps;
/// peers otherwise see a new stream on every packet and reset their jitter
/// buffers.
struct TxStream {
    ssrc: u32,
    seq: u16,
    timestamp: u32,
    encoder: Option<AudioEncoder>,
    /// Samples waiting to fill a whole packet.
    pending: Vec<i16>,
}

impl TxStream {
    fn new() -> Self {
        Self {
            ssrc: rand::random(),
            seq: rand::random(),
            timestamp: rand::random(),
            encoder: None,
            pending: Vec::new(),
        }
    }

    /// The encoder for `codec`, recreated if the codec changed.
    fn encoder(&mut self, codec: Codec) -> Result<&mut AudioEncoder> {
        if self.encoder.as_ref().map(|e| e.codec()) != Some(codec) {
            self.encoder = Some(AudioEncoder::new(codec)?);
            self.pending.clear();
        }
        Ok(self.encoder.as_mut().expect("encoder was just set"))
    }

    /// Build a packet with the current sequence number and `timestamp`, then
    /// advance the sequence number.
    fn packet(
        &mut self,
        payload_type: u8,
        marker: bool,
        timestamp: u32,
        payload: &[u8],
    ) -> Vec<u8> {
        let mut packet = Vec::with_capacity(12 + payload.len());
        packet.push(0x80); // V=2, P=0, X=0, CC=0
        packet.push(payload_type | if marker { 0x80 } else { 0 });
        packet.extend_from_slice(&self.seq.to_be_bytes());
        packet.extend_from_slice(&timestamp.to_be_bytes());
        packet.extend_from_slice(&self.ssrc.to_be_bytes());
        packet.extend_from_slice(payload);
        self.seq = self.seq.wrapping_add(1);
        packet
    }

    /// Encode one audio frame and build its packet, advancing the timestamp.
    fn audio_packet(&mut self, codec: Codec, frame: &[i16]) -> Result<Vec<u8>> {
        let payload = self.encoder(codec)?.encode(frame)?;
        let ts = self.timestamp;
        // Opus pads short frames, so advance by what was actually encoded.
        let advance = if codec == Codec::Opus {
            frame.len().max(samples_per_packet(codec))
        } else {
            frame.len()
        };
        self.timestamp = self.timestamp.wrapping_add(advance as u32);
        Ok(self.packet(codec.payload_type(), false, ts, &payload))
    }
}

/// Samples in one packet of `codec` at [`PTIME_MS`].
fn samples_per_packet(codec: Codec) -> usize {
    (codec.clock_rate() * PTIME_MS / 1000) as usize
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
    /// Outgoing stream (SSRC, sequence, timestamp, encoder)
    tx: Arc<Mutex<TxStream>>,
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
            tx: Arc::new(Mutex::new(TxStream::new())),
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
    pub fn start(&self, codec: Codec, audio_tx: Option<tokio::sync::broadcast::Sender<Vec<i16>>>) {
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
            // Every packet of one telephone-event carries the same timestamp,
            // and the end packet is sent three times. Keying on the timestamp
            // keeps those as one digit while still accepting "11".
            let mut last_dtmf_timestamp: Option<u32> = None;
            let mut decoder = match AudioDecoder::new(codec) {
                Ok(d) => d,
                Err(e) => {
                    log::error!("Cannot create {:?} decoder: {}", codec, e);
                    return;
                }
            };
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

                        if rtp.payload_type == TELEPHONE_EVENT_PT {
                            // RFC 2833 telephone-event
                            if let Some(dtmf) = parse_dtmf(rtp.payload) {
                                let mut digits = dtmf_buf.lock().await;
                                let mut events = dtmf_events.lock().await;
                                if dtmf.end
                                    && !dtmf.digit.is_whitespace()
                                    && last_dtmf_timestamp != Some(rtp.timestamp)
                                {
                                    last_dtmf_timestamp = Some(rtp.timestamp);
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
                        } else if rtp.payload_type == COMFORT_NOISE_PT
                            || !is_audio_payload(rtp.payload_type, codec)
                        {
                            continue;
                        } else {
                            // Audio packet — decode first
                            if let Ok(samples) = decoder.decode(rtp.payload) {
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

    /// Number of samples recorded so far.
    pub async fn recording_len(&self) -> usize {
        self.recording.lock().await.len()
    }

    /// Stop recording and return captured samples
    pub async fn stop_recording(&self) -> Vec<i16> {
        *self.recording_active.lock().await = false;
        self.recording.lock().await.clone()
    }

    /// Send linear PCM at the codec's clock rate as RTP to `target`.
    ///
    /// Input of any length is split into standard 20 ms packets; a partial
    /// packet is kept until the next call fills it.
    pub async fn send_audio_samples(
        &self,
        samples: &[i16],
        target: SocketAddr,
        codec: Codec,
    ) -> Result<()> {
        let frame = samples_per_packet(codec);
        let mut tx = self.tx.lock().await;
        tx.encoder(codec)?;
        tx.pending.extend_from_slice(samples);
        while tx.pending.len() >= frame {
            let chunk: Vec<i16> = tx.pending.drain(..frame).collect();
            let packet = tx.audio_packet(codec, &chunk)?;
            self.socket.send_to(&packet, target).await?;
        }
        Ok(())
    }

    /// Send a single DTMF digit (RFC 2833 telephone-event, PT=101)
    pub async fn send_dtmf_digit(
        &self,
        digit: char,
        target: SocketAddr,
        codec: Codec,
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

        let mut tx = self.tx.lock().await;
        // All packets of one event share the timestamp of its start.
        let event_timestamp = tx.timestamp;
        let event_payload = |end: bool, duration: u16| {
            let [hi, lo] = duration.to_be_bytes();
            // Volume 10 dBm0; E bit marks the end of the event.
            [event, if end { 0x8A } else { 0x0A }, hi, lo]
        };

        // Three updates 20 ms apart (durations in 8 kHz telephone-event units),
        // the first one carrying the marker bit (RFC 4733 §2.5.1.3).
        for step in 1..=3u16 {
            let packet = tx.packet(
                TELEPHONE_EVENT_PT,
                step == 1,
                event_timestamp,
                &event_payload(false, step * 160),
            );
            let _ = self.socket.send_to(&packet, target).await;
            tokio::time::sleep(std::time::Duration::from_millis(PTIME_MS as u64)).await;
        }

        // The end packet is sent three times for robustness against loss.
        for _ in 0..3 {
            let packet = tx.packet(
                TELEPHONE_EVENT_PT,
                false,
                event_timestamp,
                &event_payload(true, 480),
            );
            let _ = self.socket.send_to(&packet, target).await;
        }

        // Advance the media clock by the event plus the inter-digit gap.
        let elapsed_ms = 60 + 100;
        tx.timestamp = tx
            .timestamp
            .wrapping_add(codec.clock_rate() / 1000 * elapsed_ms);
        drop(tx);

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        Ok(())
    }

    /// Send in-band DTMF audio tones synthesized into RTP media packets (ITU-T Q.23 / RFC 4733).
    pub async fn send_dtmf_inband(
        &self,
        digit: char,
        target: SocketAddr,
        codec: Codec,
    ) -> Result<()> {
        let duration_ms = 160;
        let samples = match synthesize_dtmf_pcm(digit, duration_ms, codec.clock_rate() as usize) {
            Some(s) => s,
            None => {
                log::warn!("Invalid DTMF digit for in-band synthesis: '{}'", digit);
                return Ok(());
            }
        };

        let frame = samples_per_packet(codec);
        // The tone followed by 40 ms of silence as an inter-digit gap.
        let silence = vec![0i16; frame * 2];
        let mut tx = self.tx.lock().await;
        for chunk in samples.chunks(frame).chain(silence.chunks(frame)) {
            let packet = tx.audio_packet(codec, chunk)?;
            let _ = self.socket.send_to(&packet, target).await;
            tokio::time::sleep(std::time::Duration::from_millis(PTIME_MS as u64)).await;
        }

        Ok(())
    }
}

/// Synthesize 16-bit linear PCM samples for a DTMF digit (ITU-T Q.23 / RFC 4733).
pub fn synthesize_dtmf_pcm(
    digit: char,
    duration_ms: usize,
    sample_rate: usize,
) -> Option<Vec<i16>> {
    let (f_low, f_high): (f32, f32) = match digit.to_ascii_uppercase() {
        '1' => (697.0, 1209.0),
        '2' => (697.0, 1336.0),
        '3' => (697.0, 1477.0),
        'A' => (697.0, 1633.0),
        '4' => (770.0, 1209.0),
        '5' => (770.0, 1336.0),
        '6' => (770.0, 1477.0),
        'B' => (770.0, 1633.0),
        '7' => (852.0, 1209.0),
        '8' => (852.0, 1336.0),
        '9' => (852.0, 1477.0),
        'C' => (852.0, 1633.0),
        '*' => (941.0, 1209.0),
        '0' => (941.0, 1336.0),
        '#' => (941.0, 1477.0),
        'D' => (941.0, 1633.0),
        _ => return None,
    };

    let total_samples = (sample_rate * duration_ms) / 1000;
    let mut samples = Vec::with_capacity(total_samples);
    let amp = 8000.0f32; // Headroom to prevent clipping when summing both sines
    let pi2 = std::f32::consts::PI * 2.0;
    let rate_f = sample_rate as f32;

    for t in 0..total_samples {
        let time_sec = (t as f32) / rate_f;
        let s_low = (pi2 * f_low * time_sec).sin();
        let s_high = (pi2 * f_high * time_sec).sin();
        let val = (amp * (s_low + s_high)).clamp(-32767.0, 32767.0) as i16;
        samples.push(val);
    }

    Some(samples)
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

    #[test]
    fn test_synthesize_dtmf_pcm() {
        let pcm = synthesize_dtmf_pcm('5', 160, 8000).expect("synthesizes valid tone");
        // 160ms at 8000Hz = 1280 samples
        assert_eq!(pcm.len(), 1280);
        assert!(pcm.iter().any(|&s| s > 0));
        assert!(pcm.iter().any(|&s| s < 0));

        // Invalid digit returns None
        assert!(synthesize_dtmf_pcm('Z', 160, 8000).is_none());
    }

    /// Fields of one received RTP packet.
    #[derive(Debug)]
    struct Received {
        marker: bool,
        payload_type: u8,
        seq: u16,
        timestamp: u32,
        ssrc: u32,
        payload: Vec<u8>,
    }

    async fn receive_all(sock: &UdpSocket) -> Vec<Received> {
        let mut out = Vec::new();
        let mut buf = [0u8; 2048];
        while let Ok(Ok(n)) =
            tokio::time::timeout(std::time::Duration::from_millis(150), sock.recv(&mut buf)).await
        {
            let p = &buf[..n];
            out.push(Received {
                marker: p[1] & 0x80 != 0,
                payload_type: p[1] & 0x7F,
                seq: u16::from_be_bytes([p[2], p[3]]),
                timestamp: u32::from_be_bytes([p[4], p[5], p[6], p[7]]),
                ssrc: u32::from_be_bytes([p[8], p[9], p[10], p[11]]),
                payload: p[12..].to_vec(),
            });
        }
        out
    }

    async fn pair() -> (RtpReceiver, UdpSocket, SocketAddr) {
        let rx = RtpReceiver::bind(0).await.unwrap();
        let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = peer.local_addr().unwrap();
        (rx, peer, addr)
    }

    fn assert_one_continuous_stream(pkts: &[Received]) {
        for w in pkts.windows(2) {
            assert_eq!(w[0].ssrc, w[1].ssrc, "SSRC changed mid-stream");
            assert_eq!(w[1].seq, w[0].seq.wrapping_add(1), "sequence gap");
        }
    }

    #[tokio::test]
    async fn audio_is_split_into_20ms_packets_and_remainder_is_kept() {
        let (rx, peer, target) = pair().await;
        // 2048 samples, as the browser sends them = 12 packets + 128 left over.
        rx.send_audio_samples(&[100i16; 2048], target, Codec::Pcmu)
            .await
            .unwrap();
        // 32 more samples complete the 13th packet.
        rx.send_audio_samples(&[100i16; 32], target, Codec::Pcmu)
            .await
            .unwrap();

        let pkts = receive_all(&peer).await;
        assert_eq!(pkts.len(), 13);
        assert!(pkts
            .iter()
            .all(|p| p.payload.len() == 160 && p.payload_type == 0));
        assert_one_continuous_stream(&pkts);
        for w in pkts.windows(2) {
            assert_eq!(w[1].timestamp, w[0].timestamp.wrapping_add(160));
        }
    }

    #[tokio::test]
    async fn dtmf_events_share_the_audio_stream() {
        let (rx, peer, target) = pair().await;
        rx.send_audio_samples(&[0i16; 160], target, Codec::Pcma)
            .await
            .unwrap();
        rx.send_dtmf_digit('5', target, Codec::Pcma).await.unwrap();
        rx.send_dtmf_digit('x', target, Codec::Pcma).await.unwrap(); // ignored
        rx.send_audio_samples(&[0i16; 160], target, Codec::Pcma)
            .await
            .unwrap();

        let pkts = receive_all(&peer).await;
        assert_eq!(pkts.len(), 1 + 6 + 1);
        assert_one_continuous_stream(&pkts);

        let events = &pkts[1..7];
        assert!(events.iter().all(|p| p.payload_type == TELEPHONE_EVENT_PT));
        assert!(
            events[0].marker,
            "first event packet carries the marker bit"
        );
        assert!(events[1..].iter().all(|p| !p.marker));
        // One event = one timestamp: the start of the event.
        assert!(events.iter().all(|p| p.timestamp == events[0].timestamp));
        assert_eq!(events[0].timestamp, pkts[0].timestamp.wrapping_add(160));
        assert!(events.iter().all(|p| p.payload[0] == 5));
        assert_eq!(
            events.iter().filter(|p| p.payload[1] & 0x80 != 0).count(),
            3
        );
        // Audio resumes later on the media clock.
        assert!(pkts[7].timestamp.wrapping_sub(events[0].timestamp) >= 160 * 8);
    }

    #[tokio::test]
    async fn inband_dtmf_works_for_opus() {
        let (rx, peer, target) = pair().await;
        rx.send_dtmf_inband('#', target, Codec::Opus).await.unwrap();
        let pkts = receive_all(&peer).await;
        // 160 ms tone + 40 ms gap at 20 ms per packet.
        assert_eq!(pkts.len(), 10);
        assert!(pkts.iter().all(|p| p.payload_type == 111));
        assert_one_continuous_stream(&pkts);
        assert_eq!(pkts[1].timestamp, pkts[0].timestamp.wrapping_add(960));
    }

    /// Send a telephone-event packet from `from` to the receiver.
    async fn send_event(from: &UdpSocket, to: SocketAddr, seq: u16, ts: u32, digit: u8, end: bool) {
        let mut p = vec![0x80, TELEPHONE_EVENT_PT];
        p.extend_from_slice(&seq.to_be_bytes());
        p.extend_from_slice(&ts.to_be_bytes());
        p.extend_from_slice(&7u32.to_be_bytes());
        p.extend_from_slice(&[digit, if end { 0x8A } else { 0x0A }, 1, 0xE0]);
        from.send_to(&p, to).await.unwrap();
    }

    #[tokio::test]
    async fn repeated_digits_are_not_collapsed() {
        let rx = RtpReceiver::bind(0).await.unwrap();
        let port = rx.socket().local_addr().unwrap().port();
        let to: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        rx.start(Codec::Pcmu, None);

        let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mut seq = 0;
        // "1", "1", "2": each event sends updates then a triple end packet.
        for (ts, digit) in [(1000u32, 1u8), (3000, 1), (5000, 2)] {
            for end in [false, false, true, true, true] {
                seq += 1;
                send_event(&peer, to, seq, ts, digit, end).await;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(rx.take_dtmf().await, "112");
        rx.stop();
    }

    #[tokio::test]
    async fn non_audio_payload_types_are_not_recorded() {
        let rx = RtpReceiver::bind(0).await.unwrap();
        let port = rx.socket().local_addr().unwrap().port();
        let to: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        rx.start(Codec::Pcmu, None);
        rx.start_recording().await;

        let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        for (seq, pt) in [(1u16, 0u8), (2, COMFORT_NOISE_PT), (3, 8), (4, 0)] {
            let mut p = header(0x80, pt, seq);
            p.extend_from_slice(&[0xFF; 160]);
            peer.send_to(&p, to).await.unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(rx.stop_recording().await.len(), 320);
        rx.stop();
    }

    #[test]
    fn audio_payload_filter() {
        assert!(is_audio_payload(0, Codec::Pcmu));
        assert!(!is_audio_payload(8, Codec::Pcmu));
        assert!(is_audio_payload(8, Codec::Pcma));
        assert!(is_audio_payload(111, Codec::Opus));
        assert!(is_audio_payload(96, Codec::Opus));
        assert!(!is_audio_payload(TELEPHONE_EVENT_PT, Codec::Opus));
        assert!(!is_audio_payload(0, Codec::Opus));
    }
}
