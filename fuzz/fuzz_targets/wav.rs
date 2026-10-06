//! WAV parsing of uploaded audio files.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok((info, samples)) = rsipclient::rtp::wav::parse_wav(data) {
        assert!(info.sample_rate > 0 && info.channels > 0);
        assert!(samples.len() <= data.len());
    }
});
