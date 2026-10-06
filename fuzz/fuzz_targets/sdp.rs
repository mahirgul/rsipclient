//! SDP parsing of remote offers and answers.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rsipclient::sip::sdp;

fuzz_target!(|data: &[u8]| {
    let body = String::from_utf8_lossy(data);
    let parsed = sdp::parse_sdp(&body);
    let _ = parsed.rtp_addr();
    let _ = sdp::parse_remote_codecs(&body);
});
