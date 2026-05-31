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
/// Supports 8/16-bit mono/stereo uncompressed PCM WAV files.
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
        let chunk_end = (chunk_start + chunk_size).min(data.len());

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

            let info = WavInfo {
                sample_rate,
                channels,
                bits_per_sample,
                data_offset: chunk_start,
                data_len: chunk_end - chunk_start,
            };

            let samples: Vec<i16> = if bits_per_sample == 16 {
                data[chunk_start..chunk_end]
                    .chunks_exact(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]))
                    .collect()
            } else {
                data[chunk_start..chunk_end]
                    .iter()
                    .map(|&b| ((b as i16) - 128) * 256)
                    .collect()
            };

            return Ok((info, samples));
        }

        let skip = 8 + chunk_size + (chunk_size % 2);
        offset += skip;
    }

    anyhow::bail!("No data chunk found in WAV file")
}
