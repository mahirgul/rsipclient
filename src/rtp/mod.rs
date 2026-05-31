//! RTP (Real-time Transport Protocol) — WAV playback over RTP
//!
//! Submodules:
//! - `codec`    : Codec enum + G.711/Opus encoders/decoders
//! - `receiver` : RTP receiver + DTMF detector + WAV recorder
//! - `wav`      : WAV file parser

pub mod codec;
pub mod receiver;
pub mod wav;

use anyhow::Result;
use codec::Codec;
use std::net::SocketAddr;
use tokio::net::UdpSocket;
// Re-exports are available via rtp::codec::* and rtp::wav::* directly.

// ── RTP sender ──────────────────────────────────────────────

/// Send linear PCM samples as RTP packets using the specified codec.
///
/// Automatically resamples to the codec's native rate if needed.
/// Returns the number of RTP packets sent.
pub async fn send_wav_rtp(
    samples: &[i16],
    sample_rate: u32,
    target: SocketAddr,
    local_port: u16,
    _rtp_port: u16,
    codec: Codec,
) -> Result<usize> {
    let bind_addr: SocketAddr = format!("0.0.0.0:{}", local_port).parse()?;
    let socket = UdpSocket::bind(bind_addr).await?;
    send_wav_rtp_on_socket(&socket, samples, sample_rate, target, codec).await
}

/// Send linear PCM samples as RTP packets using the specified codec and an existing bound UDP socket.
pub async fn send_wav_rtp_on_socket(
    socket: &UdpSocket,
    samples: &[i16],
    sample_rate: u32,
    target: SocketAddr,
    codec: Codec,
) -> Result<usize> {
    let ssrc: u32 = rand::random();
    let mut seq: u16 = rand::random();
    let mut timestamp: u32 = rand::random();
    let mut packet_count = 0;

    let target_rate = codec.clock_rate();
    let samples_per_packet = (target_rate as usize * 20 / 1000).max(80);

    // Simple linear resampling if rates don't match
    let resampled: Vec<i16> = if sample_rate != target_rate {
        simple_resample(samples, sample_rate, target_rate)
    } else {
        samples.to_vec()
    };

    let start_time = tokio::time::Instant::now();
    for (i, chunk) in resampled.chunks(samples_per_packet).enumerate() {
        let payload: Vec<u8> = codec.encode(chunk)?;

        let mut packet = Vec::with_capacity(12 + payload.len());
        packet.push(0x80); // V=2, P=0, X=0, CC=0
        packet.push(codec.payload_type());
        packet.extend_from_slice(&seq.to_be_bytes());
        packet.extend_from_slice(&timestamp.to_be_bytes());
        packet.extend_from_slice(&ssrc.to_be_bytes());
        packet.extend_from_slice(&payload);

        socket.send_to(&packet, target).await?;
        packet_count += 1;
        seq = seq.wrapping_add(1);
        timestamp = timestamp.wrapping_add(chunk.len() as u32);

        // Pace the packet sending to match the real-time sample duration
        let expected_elapsed = std::time::Duration::from_secs_f64((i + 1) as f64 * 0.020);
        let actual_elapsed = start_time.elapsed();
        if actual_elapsed < expected_elapsed {
            tokio::time::sleep(expected_elapsed - actual_elapsed).await;
        }
    }

    Ok(packet_count)
}

// ── Resampling ──────────────────────────────────────────────

/// Simple linear interpolation resampling.
fn simple_resample(samples: &[i16], from_rate: u32, to_rate: u32) -> Vec<i16> {
    if from_rate == to_rate || samples.is_empty() {
        return samples.to_vec();
    }

    let ratio = from_rate as f64 / to_rate as f64;
    let out_len = (samples.len() as f64 / ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);

    for i in 0..out_len {
        let src_idx = (i as f64 * ratio) as usize;
        if src_idx + 1 < samples.len() {
            let frac = (i as f64 * ratio) - src_idx as f64;
            let a = samples[src_idx] as f64;
            let b = samples[src_idx + 1] as f64;
            out.push((a + (b - a) * frac) as i16);
        } else {
            out.push(*samples.last().unwrap_or(&0));
        }
    }

    out
}
