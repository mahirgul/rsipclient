//! Audio codec types, encoders and decoders

use anyhow::Result;

/// Audio codecs supported for RTP streaming
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Codec {
    /// G.711 μ-law, 8kHz, RTP payload type 0
    Pcmu,
    /// G.711 A-law, 8kHz, RTP payload type 8
    Pcma,
    /// Opus, 48kHz (resampled), RTP payload type 111 (dynamic)
    Opus,
}

impl Codec {
    /// Parse from config string: "pcmu", "pcma", "opus"
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "pcmu" | "g711u" | "mulaw" => Some(Codec::Pcmu),
            "pcma" | "g711a" | "alaw" => Some(Codec::Pcma),
            "opus" => Some(Codec::Opus),
            _ => None,
        }
    }

    /// RTP payload type number
    pub fn payload_type(&self) -> u8 {
        match self {
            Codec::Pcmu => 0,
            Codec::Pcma => 8,
            Codec::Opus => 111,
        }
    }

    /// Clock rate in Hz
    pub fn clock_rate(&self) -> u32 {
        match self {
            Codec::Pcmu => 8000,
            Codec::Pcma => 8000,
            Codec::Opus => 48000,
        }
    }

    /// SDP rtpmap line (without the "a=rtpmap:" prefix)
    pub fn rtpmap(&self) -> &str {
        match self {
            Codec::Pcmu => "0 PCMU/8000",
            Codec::Pcma => "8 PCMA/8000",
            Codec::Opus => "111 opus/48000/2",
        }
    }

    /// Encode a chunk of linear 16-bit PCM samples
    pub fn encode(&self, chunk: &[i16]) -> Result<Vec<u8>> {
        match self {
            Codec::Pcmu => Ok(chunk.iter().map(|&s| linear_to_mulaw(s)).collect()),
            Codec::Pcma => Ok(chunk.iter().map(|&s| linear_to_alaw(s)).collect()),
            Codec::Opus => opus_encode(chunk),
        }
    }

    /// Decode a chunk of bytes to linear 16-bit PCM samples
    pub fn decode(&self, payload: &[u8]) -> Result<Vec<i16>> {
        match self {
            Codec::Pcmu => Ok(payload.iter().map(|&b| mulaw_to_linear(b)).collect()),
            Codec::Pcma => Ok(payload.iter().map(|&b| alaw_to_linear(b)).collect()),
            Codec::Opus => opus_decode(payload),
        }
    }
}

// ── G.711 μ-law ────────────────────────────────────────────

/// Linear 16-bit PCM → G.711 μ-law
pub fn linear_to_mulaw(sample: i16) -> u8 {
    const BIAS: i16 = 0x84;
    const CLIP: i16 = 32635;

    let mut s = sample.clamp(-CLIP, CLIP);

    let sign = if s < 0 { 0x00u8 } else { 0x80u8 };
    if sign == 0x00 {
        s = -s;
    }
    s = s.saturating_add(BIAS);

    let (exp, mant) = if s > 0x1FFF {
        (7, s >> 11)
    } else if s > 0x0FFF {
        (6, s >> 9)
    } else if s > 0x07FF {
        (5, s >> 7)
    } else if s > 0x03FF {
        (4, s >> 5)
    } else if s > 0x01FF {
        (3, s >> 3)
    } else if s > 0x00FF {
        (2, s >> 1)
    } else if s > 0x007F {
        (1, s)
    } else {
        (0, s)
    };

    let magnitude = ((exp as i16) << 4) | (15 - (mant & 0x0F));
    sign | ((magnitude as u8) ^ 0x7F)
}

/// G.711 μ-law → linear 16-bit PCM
pub fn mulaw_to_linear(mulaw: u8) -> i16 {
    let m = !mulaw;
    let sign = if (m & 0x80) != 0 { 1i16 } else { -1i16 };
    let chord = ((m >> 4) & 0x07) as i16;
    let step = ((m & 0x0F) as i16) << 1;
    let value = ((step + 33) << chord) - 33;
    sign * value
}

// ── G.711 A-law ────────────────────────────────────────────

/// Linear 16-bit PCM → G.711 A-law
pub fn linear_to_alaw(sample: i16) -> u8 {
    const CLIP: i16 = 32635;

    let s = sample.clamp(-CLIP, CLIP);

    let sign = if s < 0 { 0x00u8 } else { 0x80u8 };
    let abs = s.unsigned_abs() as i16;

    let (exp, mant) = if abs >= 0x1000 {
        (7, abs >> 8)
    } else if abs >= 0x0800 {
        (6, abs >> 7)
    } else if abs >= 0x0400 {
        (5, abs >> 6)
    } else if abs >= 0x0200 {
        (4, abs >> 5)
    } else if abs >= 0x0100 {
        (3, abs >> 4)
    } else if abs >= 0x0080 {
        (2, abs >> 3)
    } else if abs >= 0x0040 {
        (1, abs >> 2)
    } else {
        (0, abs >> 1)
    };

    let alaw: i16 = if exp == 0 {
        (mant & 0x0F) >> 1
    } else {
        ((exp as i16) << 4) | (mant & 0x0F)
    };

    sign | ((alaw as u8) ^ 0x55)
}

/// G.711 A-law → linear 16-bit PCM
pub fn alaw_to_linear(alaw: u8) -> i16 {
    let a = alaw ^ 0x55;
    let sign = if (a & 0x80) != 0 { -1i16 } else { 1i16 };
    let chord = ((a >> 4) & 0x07) as i16;
    let step = (a & 0x0F) as i16;

    let value = if chord == 0 {
        (step << 1) + 1
    } else {
        ((step << 1) + 33) << chord
    };

    sign * value
}

// ── Opus ───────────────────────────────────────────────────

/// Encode audio with Opus. Falls back to PCMU when opus feature is disabled.
#[allow(unused_variables)]
pub fn opus_encode(chunk: &[i16]) -> Result<Vec<u8>> {
    #[cfg(feature = "opus")]
    {
        use opus::{Application, Channels, Encoder};
        let mut encoder = Encoder::new(48000, Channels::Mono, Application::Audio)?;
        let mut output = vec![0u8; 4000];
        let n = encoder.encode(chunk, &mut output)?;
        output.truncate(n);
        Ok(output)
    }

    #[cfg(not(feature = "opus"))]
    {
        Ok(chunk.iter().map(|&s| linear_to_mulaw(s)).collect())
    }
}

/// Decode audio with Opus. Falls back to PCMU when opus feature is disabled.
#[allow(unused_variables)]
pub fn opus_decode(payload: &[u8]) -> Result<Vec<i16>> {
    #[cfg(feature = "opus")]
    {
        use opus::{Channels, Decoder};
        let mut decoder = Decoder::new(48000, Channels::Mono)?;
        let mut output = vec![0i16; 5760]; // Max frame size
        let n = decoder.decode(payload, &mut output, false)?;
        output.truncate(n);
        Ok(output)
    }

    #[cfg(not(feature = "opus"))]
    {
        Ok(payload.iter().map(|&b| mulaw_to_linear(b)).collect())
    }
}
