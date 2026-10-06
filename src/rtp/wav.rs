//! WAV file parser — extracts linear 16-bit PCM samples

use anyhow::Result;

/// Parsed WAV file header info
#[derive(Debug)]
#[allow(dead_code)]
pub struct WavInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub data_offset: usize,
    pub data_len: usize,
}

/// Parse a WAV file and return its header info + linear 16-bit PCM samples.
/// Supports 8/16-bit mono/stereo uncompressed PCM WAV files. Multi-channel
/// audio is mixed down, so the returned samples are always mono.
pub fn parse_wav(data: &[u8]) -> Result<(WavInfo, Vec<i16>)> {
    if data.len() < 12 {
        anyhow::bail!("File too small to be a valid WAV");
    }

    if &data[0..4] != b"RIFF" {
        anyhow::bail!("Not a RIFF/WAV file");
    }
    if &data[8..12] != b"WAVE" {
        anyhow::bail!("Not a WAV file");
    }

    let mut channels = None;
    let mut sample_rate = None;
    let mut bits_per_sample = None;
    let mut fmt_seen = false;

    let mut offset = 12;
    while offset + 8 <= data.len() {
        let chunk_id = &data[offset..offset + 4];
        let chunk_size = u32::from_le_bytes([
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]) as usize;

        let chunk_start = offset + 8;
        // Saturate: a hostile size must not overflow `usize` on 32-bit targets.
        let chunk_end = chunk_start.saturating_add(chunk_size).min(data.len());

        if chunk_id == b"fmt " {
            if chunk_end - chunk_start < 16 {
                anyhow::bail!("fmt chunk too small");
            }
            let audio_format = u16::from_le_bytes([data[chunk_start], data[chunk_start + 1]]);
            if audio_format != 1 {
                anyhow::bail!(
                    "Only uncompressed PCM WAV supported (format={})",
                    audio_format
                );
            }
            channels = Some(u16::from_le_bytes([
                data[chunk_start + 2],
                data[chunk_start + 3],
            ]));
            sample_rate = Some(u32::from_le_bytes([
                data[chunk_start + 4],
                data[chunk_start + 5],
                data[chunk_start + 6],
                data[chunk_start + 7],
            ]));
            bits_per_sample = Some(u16::from_le_bytes([
                data[chunk_start + 14],
                data[chunk_start + 15],
            ]));
            fmt_seen = true;
        } else if chunk_id == b"data" {
            if !fmt_seen {
                anyhow::bail!("data chunk appeared before fmt chunk");
            }
            let channels = channels.unwrap();
            let sample_rate = sample_rate.unwrap();
            let bits_per_sample = bits_per_sample.unwrap();

            if bits_per_sample != 8 && bits_per_sample != 16 {
                anyhow::bail!("Only 8-bit and 16-bit PCM WAV supported");
            }
            // Playback divides by both; zero would panic further down.
            if channels == 0 || sample_rate == 0 {
                anyhow::bail!(
                    "Invalid WAV format (channels={}, sample_rate={})",
                    channels,
                    sample_rate
                );
            }

            let info = WavInfo {
                sample_rate,
                channels,
                bits_per_sample,
                data_offset: chunk_start,
                data_len: chunk_end - chunk_start,
            };

            let samples: Vec<i16> = if bits_per_sample == 16 {
                data[chunk_start..chunk_end]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| i16::from_le_bytes(*c))
                    .collect()
            } else {
                data[chunk_start..chunk_end]
                    .iter()
                    .map(|&b| ((b as i16) - 128) * 256)
                    .collect()
            };

            return Ok((info, downmix(samples, channels)));
        }

        let skip = chunk_size.saturating_add(8 + chunk_size % 2);
        offset = offset.saturating_add(skip);
    }

    anyhow::bail!("No data chunk found in WAV file")
}

/// Average interleaved multi-channel samples into a single mono channel.
fn downmix(samples: Vec<i16>, channels: u16) -> Vec<i16> {
    let channels = channels as usize;
    if channels <= 1 {
        return samples;
    }
    samples
        .chunks_exact(channels)
        .map(|frame| {
            let sum: i32 = frame.iter().map(|&s| s as i32).sum();
            (sum / channels as i32) as i16
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a canonical PCM WAV file in memory.
    fn wav_bytes(channels: u16, sample_rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        let block_align = channels * bits / 8;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&(sample_rate * block_align as u32).to_le_bytes());
        out.extend_from_slice(&block_align.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        out
    }

    fn pcm16(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    #[test]
    fn parses_16_bit_mono() {
        let data = wav_bytes(1, 8000, 16, &pcm16(&[0, 1000, -1000, i16::MAX]));
        let (info, samples) = parse_wav(&data).unwrap();
        assert_eq!(info.sample_rate, 8000);
        assert_eq!(info.channels, 1);
        assert_eq!(info.bits_per_sample, 16);
        assert_eq!(info.data_offset, 44);
        assert_eq!(info.data_len, 8);
        assert_eq!(samples, vec![0, 1000, -1000, i16::MAX]);
    }

    #[test]
    fn parses_8_bit_unsigned_samples() {
        let data = wav_bytes(1, 8000, 8, &[128, 255, 0]);
        let (_, samples) = parse_wav(&data).unwrap();
        assert_eq!(samples, vec![0, 127 * 256, -128 * 256]);
    }

    #[test]
    fn stereo_is_mixed_down_to_mono() {
        let data = wav_bytes(2, 16000, 16, &pcm16(&[100, 300, -50, -150, 7, 7]));
        let (info, samples) = parse_wav(&data).unwrap();
        assert_eq!(info.channels, 2);
        assert_eq!(samples, vec![200, -100, 7]);
    }

    #[test]
    fn skips_unknown_chunks_including_odd_padding() {
        let mut data = wav_bytes(1, 8000, 16, &pcm16(&[42]));
        // Insert a 3-byte LIST chunk (padded to 4) between fmt and data.
        let extra = [b'L', b'I', b'S', b'T', 3, 0, 0, 0, 1, 2, 3, 0];
        data.splice(36..36, extra);
        let (_, samples) = parse_wav(&data).unwrap();
        assert_eq!(samples, vec![42]);
    }

    #[test]
    fn truncated_data_chunk_is_clamped_to_the_file() {
        let mut data = wav_bytes(1, 8000, 16, &pcm16(&[1, 2, 3]));
        // Claim far more data than the file holds.
        data[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        let (info, samples) = parse_wav(&data).unwrap();
        assert_eq!(info.data_len, 6);
        assert_eq!(samples, vec![1, 2, 3]);
    }

    #[test]
    fn huge_unknown_chunk_size_does_not_overflow() {
        let mut data = wav_bytes(1, 8000, 16, &pcm16(&[1]));
        data.splice(36..36, *b"junk\xff\xff\xff\xff");
        assert!(parse_wav(&data).is_err());
    }

    #[test]
    fn rejects_zero_sample_rate_and_channels() {
        assert!(parse_wav(&wav_bytes(1, 0, 16, &pcm16(&[1]))).is_err());
        assert!(parse_wav(&wav_bytes(0, 8000, 16, &pcm16(&[1]))).is_err());
    }

    #[test]
    fn rejects_malformed_files() {
        assert!(parse_wav(b"").is_err());
        assert!(parse_wav(b"RIFF\0\0\0\0WAVX").is_err());
        assert!(parse_wav(b"RIFX\0\0\0\0WAVE").is_err());
        // Header only, no chunks.
        assert!(parse_wav(b"RIFF\0\0\0\0WAVE").is_err());
        // 24-bit is not supported.
        assert!(parse_wav(&wav_bytes(1, 8000, 24, &[0; 6])).is_err());
        // Compressed formats are rejected.
        let mut adpcm = wav_bytes(1, 8000, 16, &[0; 4]);
        adpcm[20] = 2;
        assert!(parse_wav(&adpcm).is_err());
        // data before fmt.
        let mut out = b"RIFF\0\0\0\0WAVEdata\x02\0\0\0\0\0".to_vec();
        out.extend_from_slice(&wav_bytes(1, 8000, 16, &[])[12..]);
        assert!(parse_wav(&out).is_err());
        // fmt chunk shorter than 16 bytes.
        assert!(parse_wav(b"RIFF\0\0\0\0WAVEfmt \x04\0\0\0\x01\0\x01\0").is_err());
    }

    #[test]
    fn round_trips_through_save_wav() {
        let dir = std::env::temp_dir().join(format!("rsip-wav-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rt.wav");
        let samples: Vec<i16> = (0..400).map(|i| (i * 80 - 16000) as i16).collect();

        crate::rtp::receiver::save_wav(&samples, 8000, path.to_str().unwrap()).unwrap();
        let (info, parsed) = parse_wav(&std::fs::read(&path).unwrap()).unwrap();
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(info.sample_rate, 8000);
        assert_eq!(info.channels, 1);
        assert_eq!(parsed, samples);
    }
}
