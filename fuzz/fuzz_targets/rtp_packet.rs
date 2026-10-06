//! RTP header parsing and G.711 decoding of whatever arrives on the RTP port.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rsipclient::rtp::codec::Codec;
use rsipclient::rtp::receiver::parse_rtp;

fuzz_target!(|data: &[u8]| {
    if let Some(rtp) = parse_rtp(data) {
        assert!(rtp.payload.len() <= data.len());
        let _ = Codec::Pcmu.decode(rtp.payload);
        let _ = Codec::Pcma.decode(rtp.payload);
    }
});
