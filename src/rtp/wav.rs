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
    if data.len() < 44 {
        anyhow::bail!("File too small to be a valid WAV");
    }

    if &data[0..4] != b"RIFF" {
        anyhow::bail!("Not a RIFF/WAV file");
    }
    if &data[8..12] != b"WAVE" {
        anyhow::bail!("Not a WAV file");
    }
    if &data[12..16] != b"fmt " {
        anyhow::bail!("Missing fmt chunk");
    }

    let fmt_size = u32::from_le_bytes([data[16], data[17], data[18], data[19]]) as usize;
    let audio_format = u16::from_le_bytes([data[20], data[21]]);
    if audio_format != 1 {
        anyhow::bail!(
            "Only uncompressed PCM WAV supported (format={})",
            audio_format
        );
    }

    let channels = u16::from_le_bytes([data[22], data[23]]);
    let sample_rate = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);
    let bits_per_sample = u16::from_le_bytes([data[34], data[35]]);

    if bits_per_sample != 8 && bits_per_sample != 16 {
        anyhow::bail!("Only 8-bit and 16-bit PCM WAV supported");
    }

    // Find data chunk
    let mut offset = 20 + fmt_size;
    while offset + 8 < data.len() {
        let chunk_id = &data[offset..offset + 4];
        let chunk_size = u32::from_le_bytes([
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]) as usize;

        if chunk_id == b"data" {
            let data_start = offset + 8;
            let data_end = (data_start + chunk_size).min(data.len());

            let info = WavInfo {
                sample_rate,
                channels,
                bits_per_sample,
                data_offset: data_start,
                data_len: data_end - data_start,
            };

            let samples: Vec<i16> = if bits_per_sample == 16 {
                data[data_start..data_end]
                    .chunks_exact(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]))
                    .collect()
            } else {
                data[data_start..data_end]
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
