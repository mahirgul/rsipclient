//! SIP message framing on TCP/TLS byte streams.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rsipclient::sip::transport::extract_sip_message;

fuzz_target!(|data: &[u8]| {
    let mut buf = data.to_vec();
    let mut previous = buf.len() + 1;
    while let Some(msg) = extract_sip_message(&mut buf) {
        assert!(!msg.is_empty());
        // Every extracted message must consume input.
        assert!(buf.len() < previous);
        previous = buf.len();
    }
});
