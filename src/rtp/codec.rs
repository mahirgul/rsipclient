//! Audio codec types, encoders and decoders

use anyhow::Result;

/// Audio codecs supported for RTP streaming
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    pub fn from_name(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "pcmu" | "g711u" | "mulaw" => Some(Codec::Pcmu),
            "pcma" | "g711a" | "alaw" => Some(Codec::Pcma),
            "opus" => Some(Codec::Opus),
            _ => None,
        }
    }

    /// Convert back to config string
    pub fn to_config_str(self) -> &'static str {
        match self {
            Codec::Pcmu => "pcmu",
            Codec::Pcma => "pcma",
            Codec::Opus => "opus",
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

    /// Map an RTP payload type number back to a codec (static assignments plus
    /// our dynamic Opus assignment).
    pub fn from_payload_type(pt: u8) -> Option<Self> {
        match pt {
            0 => Some(Codec::Pcmu),
            8 => Some(Codec::Pcma),
            111 => Some(Codec::Opus),
            _ => None,
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
//
// Segment-based G.711 conversion following ITU-T G.711 and its classic public
// domain reference implementation (Sun Microsystems g711.c). Peers decode with
// exactly these tables, so any deviation is heard as distortion.

/// Bias added to the magnitude before μ-law segment lookup (16-bit scale).
const ULAW_BIAS: i32 = 0x84;
/// Largest 14-bit magnitude μ-law encodes before clipping.
const ULAW_CLIP: i32 = 8159;

/// Linear 16-bit PCM → G.711 μ-law
pub fn linear_to_mulaw(sample: i16) -> u8 {
    // μ-law works on 14-bit samples.
    let pcm = (sample as i32) >> 2;
    // The sign bit ends up set for positive samples after the final inversion.
    let (magnitude, mask) = if pcm < 0 {
        (-pcm, 0x7Fu8)
    } else {
        (pcm, 0xFFu8)
    };
    let biased = magnitude.min(ULAW_CLIP) + (ULAW_BIAS >> 2);

    // Segment boundaries are 0x3F, 0x7F, ..., 0x1FFF.
    let segment = if biased < 0x40 {
        0
    } else {
        biased.ilog2() as i32 - 5
    };
    if segment > 7 {
        return 0x7F ^ mask;
    }
    let mantissa = (biased >> (segment + 1)) & 0x0F;
    (((segment << 4) | mantissa) as u8) ^ mask
}

/// G.711 μ-law → linear 16-bit PCM
pub fn mulaw_to_linear(mulaw: u8) -> i16 {
    let u = !mulaw;
    let segment = ((u & 0x70) >> 4) as i32;
    let t = ((((u & 0x0F) as i32) << 3) + ULAW_BIAS) << segment;
    let value = if u & 0x80 != 0 {
        ULAW_BIAS - t
    } else {
        t - ULAW_BIAS
    };
    value as i16
}

// ── G.711 A-law ────────────────────────────────────────────

/// Linear 16-bit PCM → G.711 A-law
pub fn linear_to_alaw(sample: i16) -> u8 {
    // A-law works on 13-bit samples.
    let pcm = (sample as i32) >> 3;
    // Even bits are inverted (0x55); the sign bit is set for positive samples.
    let (magnitude, mask) = if pcm >= 0 {
        (pcm, 0xD5u8)
    } else {
        (-pcm - 1, 0x55u8)
    };

    // Segment boundaries are 0x1F, 0x3F, ..., 0xFFF; a 13-bit magnitude never
    // exceeds the last one.
    let segment = if magnitude < 0x20 {
        0
    } else {
        magnitude.ilog2() as i32 - 4
    };
    let mantissa = if segment < 2 {
        (magnitude >> 1) & 0x0F
    } else {
        (magnitude >> segment) & 0x0F
    };
    (((segment << 4) | mantissa) as u8) ^ mask
}

/// G.711 A-law → linear 16-bit PCM
pub fn alaw_to_linear(alaw: u8) -> i16 {
    let a = alaw ^ 0x55;
    let segment = ((a & 0x70) >> 4) as i32;
    let mut t = ((a & 0x0F) as i32) << 4;
    match segment {
        0 => t += 8,
        1 => t += 0x108,
        _ => t = (t + 0x108) << (segment - 1),
    }
    let value = if a & 0x80 != 0 { t } else { -t };
    value as i16
}

// ── Opus ───────────────────────────────────────────────────

/// Frame sizes (in samples at 48 kHz) that Opus accepts: 2.5 to 60 ms.
const OPUS_FRAME_SIZES: [usize; 6] = [120, 240, 480, 960, 1920, 2880];
/// Largest frame Opus can decode (120 ms at 48 kHz).
const OPUS_MAX_DECODED: usize = 5760;
/// Upper bound on one encoded Opus packet.
const OPUS_MAX_PACKET: usize = 4000;

/// Zero-pad `chunk` up to the next frame size Opus accepts.
///
/// The last chunk of a stream is rarely a whole frame, and Opus rejects any
/// length that is not one of its fixed frame sizes.
fn opus_frame(chunk: &[i16]) -> Result<std::borrow::Cow<'_, [i16]>> {
    let Some(&size) = OPUS_FRAME_SIZES.iter().find(|&&s| s >= chunk.len()) else {
        anyhow::bail!(
            "Opus frame too long: {} samples (max {})",
            chunk.len(),
            OPUS_FRAME_SIZES[OPUS_FRAME_SIZES.len() - 1]
        );
    };
    if size == chunk.len() {
        return Ok(std::borrow::Cow::Borrowed(chunk));
    }
    let mut padded = chunk.to_vec();
    padded.resize(size, 0);
    Ok(std::borrow::Cow::Owned(padded))
}

/// Encode audio with a one-off Opus encoder.
///
/// Prefer [`AudioEncoder`] for streams: Opus is a predictive codec, and a
/// fresh encoder per packet both wastes CPU and degrades quality.
pub fn opus_encode(chunk: &[i16]) -> Result<Vec<u8>> {
    AudioEncoder::new(Codec::Opus)?.encode(chunk)
}

/// Decode audio with a one-off Opus decoder.
///
/// Prefer [`AudioDecoder`] for streams, for the same reason as [`opus_encode`].
pub fn opus_decode(payload: &[u8]) -> Result<Vec<i16>> {
    AudioDecoder::new(Codec::Opus)?.decode(payload)
}

// ── Stateful per-stream coders ─────────────────────────────

/// Encoder for one outgoing audio stream.
///
/// G.711 is stateless, but Opus keeps prediction state between frames, so one
/// encoder must live for the whole stream.
pub struct AudioEncoder {
    codec: Codec,
    opus: Option<opus::Encoder>,
}

impl AudioEncoder {
    pub fn new(codec: Codec) -> Result<Self> {
        let opus = match codec {
            Codec::Opus => Some(opus::Encoder::new(
                48000,
                opus::Channels::Mono,
                opus::Application::Voip,
            )?),
            Codec::Pcmu | Codec::Pcma => None,
        };
        Ok(Self { codec, opus })
    }

    pub fn codec(&self) -> Codec {
        self.codec
    }

    /// Encode one frame of linear PCM at the codec's clock rate.
    ///
    /// Opus input shorter than a valid frame is zero-padded.
    pub fn encode(&mut self, chunk: &[i16]) -> Result<Vec<u8>> {
        match self.opus.as_mut() {
            Some(encoder) => {
                let frame = opus_frame(chunk)?;
                let mut output = vec![0u8; OPUS_MAX_PACKET];
                let n = encoder.encode(&frame, &mut output)?;
                output.truncate(n);
                Ok(output)
            }
            None => self.codec.encode(chunk),
        }
    }
}

/// Decoder for one incoming audio stream (see [`AudioEncoder`]).
pub struct AudioDecoder {
    codec: Codec,
    opus: Option<opus::Decoder>,
}

impl AudioDecoder {
    pub fn new(codec: Codec) -> Result<Self> {
        let opus = match codec {
            Codec::Opus => Some(opus::Decoder::new(48000, opus::Channels::Mono)?),
            Codec::Pcmu | Codec::Pcma => None,
        };
        Ok(Self { codec, opus })
    }

    pub fn codec(&self) -> Codec {
        self.codec
    }

    /// Decode one RTP payload to linear PCM at the codec's clock rate.
    pub fn decode(&mut self, payload: &[u8]) -> Result<Vec<i16>> {
        match self.opus.as_mut() {
            Some(decoder) => {
                let mut output = vec![0i16; OPUS_MAX_DECODED];
                let n = decoder.decode(payload, &mut output, false)?;
                output.truncate(n);
                Ok(output)
            }
            None => self.codec.decode(payload),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Well-known G.711 code words and the linear values every compliant
    /// decoder produces for them.
    #[test]
    fn mulaw_decodes_reference_code_words() {
        assert_eq!(mulaw_to_linear(0xFF), 0);
        assert_eq!(mulaw_to_linear(0x7F), 0);
        assert_eq!(mulaw_to_linear(0x80), 32124);
        assert_eq!(mulaw_to_linear(0x00), -32124);
        assert_eq!(mulaw_to_linear(0xD5), 716);
        assert_eq!(mulaw_to_linear(0x55), -716);
    }

    #[test]
    fn alaw_decodes_reference_code_words() {
        assert_eq!(alaw_to_linear(0xD5), 8);
        assert_eq!(alaw_to_linear(0x55), -8);
        assert_eq!(alaw_to_linear(0xAA), 32256);
        assert_eq!(alaw_to_linear(0x2A), -32256);
        assert_eq!(alaw_to_linear(0xFF), 848);
        assert_eq!(alaw_to_linear(0x7F), -848);
    }

    #[test]
    fn silence_encodes_to_the_standard_idle_pattern() {
        assert_eq!(linear_to_mulaw(0), 0xFF);
        assert_eq!(linear_to_alaw(0), 0xD5);
    }

    #[test]
    fn full_scale_is_clipped_not_wrapped() {
        assert_eq!(linear_to_mulaw(i16::MAX), 0x80);
        assert_eq!(linear_to_mulaw(i16::MIN), 0x00);
        assert_eq!(linear_to_alaw(i16::MAX), 0xAA);
        assert_eq!(linear_to_alaw(i16::MIN), 0x2A);
    }

    #[test]
    fn every_code_word_survives_a_decode_encode_round_trip() {
        for b in 0..=255u8 {
            // 0x7F is μ-law "negative zero"; it re-encodes as positive zero.
            let expected = if b == 0x7F { 0xFF } else { b };
            assert_eq!(
                linear_to_mulaw(mulaw_to_linear(b)),
                expected,
                "μ-law {b:#04x}"
            );
            assert_eq!(linear_to_alaw(alaw_to_linear(b)), b, "A-law {b:#04x}");
        }
    }

    #[test]
    fn encoding_is_monotonic_and_close_to_the_input() {
        let mut prev_u = i16::MIN;
        let mut prev_a = i16::MIN;
        for x in (i16::MIN..=i16::MAX).step_by(7) {
            let u = mulaw_to_linear(linear_to_mulaw(x));
            let a = alaw_to_linear(linear_to_alaw(x));
            assert!(u >= prev_u, "μ-law not monotonic at {x}");
            assert!(a >= prev_a, "A-law not monotonic at {x}");
            prev_u = u;
            prev_a = a;

            // Quantisation error stays within half a step of the top segment.
            let tolerance = 1024 + (x as i32).abs() / 16;
            assert!(
                (u as i32 - x as i32).abs() <= tolerance,
                "μ-law error at {x}: {u}"
            );
            assert!(
                (a as i32 - x as i32).abs() <= tolerance,
                "A-law error at {x}: {a}"
            );
        }
    }

    #[test]
    fn g711_encode_and_decode_preserve_length() {
        let pcm: Vec<i16> = (0..160).map(|i| (i * 200 - 16000) as i16).collect();
        for codec in [Codec::Pcmu, Codec::Pcma] {
            let encoded = codec.encode(&pcm).unwrap();
            assert_eq!(encoded.len(), pcm.len());
            assert_eq!(codec.decode(&encoded).unwrap().len(), pcm.len());
        }
    }

    #[test]
    fn opus_round_trip_produces_a_full_frame() {
        let pcm: Vec<i16> = (0..960)
            .map(|i| ((i as f32 * 0.05).sin() * 8000.0) as i16)
            .collect();
        let encoded = Codec::Opus.encode(&pcm).unwrap();
        assert!(!encoded.is_empty());
        assert_eq!(Codec::Opus.decode(&encoded).unwrap().len(), 960);
    }

    #[test]
    fn codec_names_and_payload_types_round_trip() {
        for codec in [Codec::Pcmu, Codec::Pcma, Codec::Opus] {
            assert_eq!(Codec::from_name(codec.to_config_str()), Some(codec));
            assert_eq!(Codec::from_payload_type(codec.payload_type()), Some(codec));
            assert!(codec
                .rtpmap()
                .starts_with(&codec.payload_type().to_string()));
        }
        assert_eq!(Codec::from_name("G711U"), Some(Codec::Pcmu));
        assert_eq!(Codec::from_name("alaw"), Some(Codec::Pcma));
        assert_eq!(Codec::from_name("g729"), None);
        assert_eq!(Codec::from_payload_type(18), None);
        assert_eq!(Codec::Opus.clock_rate(), 48000);
        assert_eq!(Codec::Pcma.clock_rate(), 8000);
    }

    #[test]
    fn opus_pads_short_frames_and_rejects_oversized_ones() {
        // 100 samples is not an Opus frame size; it is padded to 120.
        let encoded = Codec::Opus.encode(&[500i16; 100]).unwrap();
        assert_eq!(Codec::Opus.decode(&encoded).unwrap().len(), 120);
        // The tail of a 20 ms stream padded to a full 20 ms frame.
        let encoded = Codec::Opus.encode(&[500i16; 700]).unwrap();
        assert_eq!(Codec::Opus.decode(&encoded).unwrap().len(), 960);
        assert!(Codec::Opus.encode(&vec![0i16; 2881]).is_err());
    }

    #[test]
    fn stateful_opus_stream_reproduces_a_tone() {
        let tone: Vec<i16> = (0..960 * 10)
            .map(|i| {
                ((i as f32 * 2.0 * std::f32::consts::PI * 440.0 / 48000.0).sin() * 10000.0) as i16
            })
            .collect();
        let mut enc = AudioEncoder::new(Codec::Opus).unwrap();
        let mut dec = AudioDecoder::new(Codec::Opus).unwrap();
        assert_eq!(enc.codec(), Codec::Opus);
        assert_eq!(dec.codec(), Codec::Opus);

        let mut out = Vec::new();
        for frame in tone.chunks(960) {
            let packet = enc.encode(frame).unwrap();
            out.extend(dec.decode(&packet).unwrap());
        }
        assert_eq!(out.len(), tone.len());
        // After the codec has settled, the decoded energy matches the input.
        let energy =
            |s: &[i16]| s.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / s.len() as f64;
        let ratio = energy(&out[4800..]) / energy(&tone[4800..]);
        assert!((0.5..2.0).contains(&ratio), "energy ratio {ratio}");
    }

    #[test]
    fn stateful_g711_coders_match_the_stateless_path() {
        let pcm: Vec<i16> = (0..160).map(|i| (i * 150 - 12000) as i16).collect();
        for codec in [Codec::Pcmu, Codec::Pcma] {
            let mut enc = AudioEncoder::new(codec).unwrap();
            let mut dec = AudioDecoder::new(codec).unwrap();
            let packet = enc.encode(&pcm).unwrap();
            assert_eq!(packet, codec.encode(&pcm).unwrap());
            assert_eq!(dec.decode(&packet).unwrap(), codec.decode(&packet).unwrap());
        }
    }
}
