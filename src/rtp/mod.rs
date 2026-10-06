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
use std::sync::Arc;
use tokio::net::UdpSocket;
// Re-exports are available via rtp::codec::* and rtp::wav::* directly.

/// Helper function to play a WAV file asynchronously over RTP.
pub async fn play_wav_file(
    file_path: &str,
    socket_opt: Option<Arc<UdpSocket>>,
    target: SocketAddr,
    codec: Codec,
    rtp_port: u16,
) -> Result<(usize, u32)> {
    let data = tokio::fs::read(file_path).await?;
    let (info, samples) = crate::rtp::wav::parse_wav(&data)?;
    let sample_count = samples.len();
    let sample_rate = info.sample_rate;

    tokio::spawn(async move {
        let res = if let Some(socket) = socket_opt {
            send_wav_rtp_on_socket(&socket, &samples, sample_rate, target, codec).await
        } else {
            send_wav_rtp(&samples, sample_rate, target, 0, rtp_port, codec).await
        };
        match res {
            Ok(n) => log::info!("Sent {} RTP packets (codec={:?})", n, codec),
            Err(e) => log::error!("RTP send error: {}", e),
        }
    });

    Ok((sample_count, sample_rate))
}

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
            out.push((a + (b - a) * frac).round() as i16);
        } else {
            out.push(*samples.last().unwrap_or(&0));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_is_identity_for_equal_rates_and_empty_input() {
        assert_eq!(simple_resample(&[1, 2, 3], 8000, 8000), vec![1, 2, 3]);
        assert!(simple_resample(&[], 16000, 8000).is_empty());
    }

    #[test]
    fn downsampling_halves_the_length_and_keeps_samples() {
        let input: Vec<i16> = (0..160).collect();
        let out = simple_resample(&input, 16000, 8000);
        assert_eq!(out.len(), 80);
        assert_eq!(out[0], 0);
        assert_eq!(out[10], 20);
        assert_eq!(out[79], 158);
    }

    #[test]
    fn upsampling_interpolates_between_neighbours() {
        let out = simple_resample(&[0, 600, 1200], 8000, 48000);
        assert_eq!(out.len(), 18);
        assert_eq!(&out[..7], &[0, 100, 200, 300, 400, 500, 600]);
        // Samples past the last input hold the final value.
        assert_eq!(*out.last().unwrap(), 1200);
    }

    #[tokio::test]
    async fn sends_well_formed_rtp_packets() {
        let rx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let tx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let target = rx.local_addr().unwrap();

        // 50 ms of 16 kHz audio → 400 samples at 8 kHz → 3 packets (160+160+80).
        let samples = vec![1000i16; 800];
        let sent = send_wav_rtp_on_socket(&tx, &samples, 16000, target, Codec::Pcma)
            .await
            .unwrap();
        assert_eq!(sent, 3);

        let mut buf = [0u8; 1500];
        let mut headers = Vec::new();
        for _ in 0..sent {
            let n = rx.recv(&mut buf).await.unwrap();
            let pkt = crate::rtp::receiver::parse_rtp(&buf[..n]).unwrap();
            assert_eq!(pkt.payload_type, 8);
            let ts = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
            let ssrc = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);
            headers.push((pkt.sequence, ts, ssrc, pkt.payload.len()));
        }
        assert_eq!(headers[0].3, 160);
        assert_eq!(headers[2].3, 80);
        assert!(
            headers.iter().all(|h| h.2 == headers[0].2),
            "SSRC must be stable"
        );
        assert_eq!(headers[1].0, headers[0].0.wrapping_add(1));
        assert_eq!(headers[2].0, headers[1].0.wrapping_add(1));
        assert_eq!(headers[1].1, headers[0].1.wrapping_add(160));
        assert_eq!(headers[2].1, headers[1].1.wrapping_add(160));
    }
}
