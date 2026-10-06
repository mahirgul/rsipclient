//! WAV → RTP → receiver → WAV round trip over real UDP sockets.

use rsipclient::rtp::codec::Codec;
use rsipclient::rtp::receiver::{save_wav, RtpReceiver};
use rsipclient::rtp::wav::parse_wav;
use std::time::Duration;

async fn round_trip(codec: Codec, wav_rate: u32) -> (Vec<i16>, Vec<i16>) {
    let dir = std::env::temp_dir().join(format!("rsip-media-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("tone.wav");

    // 200 ms of a 400 Hz tone.
    let n = wav_rate as usize / 5;
    let tone: Vec<i16> = (0..n)
        .map(|i| {
            ((i as f32 * 2.0 * std::f32::consts::PI * 400.0 / wav_rate as f32).sin() * 12000.0)
                as i16
        })
        .collect();
    save_wav(&tone, wav_rate, wav.to_str().unwrap()).unwrap();

    let rx = RtpReceiver::bind(0).await.unwrap();
    let port = rx.socket().local_addr().unwrap().port();
    rx.start(codec, None);
    rx.start_recording().await;

    let target = format!("127.0.0.1:{port}").parse().unwrap();
    let (count, rate) =
        rsipclient::rtp::play_wav_file(wav.to_str().unwrap(), None, target, codec, 0)
            .await
            .unwrap();
    assert_eq!((count, rate), (n, wav_rate));

    // Wait until the stream has drained: the recording stops growing.
    let mut last_len = usize::MAX;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(150)).await;
        let len = rx.recording_len().await;
        if len > 0 && len == last_len {
            break;
        }
        last_len = len;
    }
    let recorded = rx.stop_recording().await;
    rx.stop();

    let out = dir.join("rec.wav");
    save_wav(&recorded, codec.clock_rate(), out.to_str().unwrap()).unwrap();
    let (info, reread) = parse_wav(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(info.sample_rate, codec.clock_rate());
    std::fs::remove_dir_all(&dir).ok();
    (tone, reread)
}

fn rms(s: &[i16]) -> f64 {
    (s.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / s.len().max(1) as f64).sqrt()
}

#[tokio::test]
async fn pcmu_round_trip_preserves_the_signal() {
    let (sent, got) = round_trip(Codec::Pcmu, 8000).await;
    assert_eq!(got.len(), sent.len());
    // G.711 is near-transparent: per-sample error stays small.
    let max_err = sent
        .iter()
        .zip(&got)
        .map(|(a, b)| (*a as i32 - *b as i32).abs())
        .max()
        .unwrap();
    assert!(max_err < 600, "max error {max_err}");
}

#[tokio::test]
async fn pcma_round_trip_resamples_16k_input() {
    let (sent, got) = round_trip(Codec::Pcma, 16000).await;
    assert_eq!(got.len(), sent.len() / 2);
    let ratio = rms(&got) / rms(&sent);
    assert!((0.9..1.1).contains(&ratio), "level ratio {ratio}");
}

#[tokio::test]
async fn opus_round_trip_keeps_the_level() {
    let (sent, got) = round_trip(Codec::Opus, 8000).await;
    // 1600 samples at 8 kHz → 9600 at 48 kHz, sent as 10 whole 20 ms frames.
    assert_eq!(got.len(), 9600);
    let ratio = rms(&got[2000..]) / rms(&sent[400..]);
    assert!((0.6..1.5).contains(&ratio), "level ratio {ratio}");
}
