//! Randomized robustness tests: every parser that sees network or upload
//! input must reject garbage without panicking. Seeds are fixed so failures
//! reproduce; `cargo fuzz` (see `fuzz/`) explores far more of the space.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rsipclient::sip::{sdp, utils};

const ITERATIONS: usize = 3000;

const SAMPLE_SIP: &str = "SIP/2.0 401 Unauthorized\r\n\
    Via: SIP/2.0/UDP 10.0.0.5:5060;branch=z9hG4bK-1;rport=5060\r\n\
    From: \"Ali\" <sip:1001@example.com>;tag=abc\r\n\
    To: <sip:1001@example.com>;tag=xyz\r\n\
    Call-ID: 123@example.com\r\n\
    CSeq: 2 REGISTER\r\n\
    Record-Route: <sip:p1.example.com;lr>, <sip:p2.example.com;lr>\r\n\
    WWW-Authenticate: Digest realm=\"example.com\", nonce=\"n\", qop=\"auth,auth-int\", opaque=\"o\", stale=TRUE\r\n\
    Proxy-Authenticate: Digest realm=\"proxy\", nonce=\"m\", algorithm=MD5\r\n\
    Session-Expires: 1800;refresher=uas\r\n\
    Min-SE: 90\r\n\
    RSeq: 7\r\n\
    RAck: 7 2 INVITE\r\n\
    Contact: <sip:bob@10.0.0.9:5060;transport=udp>\r\n\
    Content-Type: application/sdp\r\n\
    Content-Length: 120\r\n\
    \r\n\
    v=0\r\no=- 0 0 IN IP4 1.2.3.4\r\nc=IN IP4 1.2.3.4\r\nm=audio 4000 RTP/SAVP 0 8 111 101\r\n\
    a=rtpmap:111 opus/48000/2\r\na=rtcp:4001\r\na=crypto:1 AES_CM_128_HMAC_SHA1_80 inline:abc\r\na=recvonly\r\n";

/// Random edits of `seed`: byte flips, insertions of syntax characters and
/// multi-byte UTF-8, deletions and truncation.
fn mutate(rng: &mut StdRng, seed: &[u8]) -> Vec<u8> {
    const TOKENS: &[&[u8]] = &[
        b"\r\n",
        b"\n",
        b":",
        b";",
        b",",
        b"=",
        b"\"",
        b"<",
        b">",
        b"@",
        b" ",
        b"tag=",
        b"Digest ",
        "ğüşİ".as_bytes(),
        "\u{1F4DE}".as_bytes(),
        b"\0",
        b"\xff",
        b"99999999999999999999",
    ];
    let mut data = seed.to_vec();
    for _ in 0..rng.gen_range(1..12) {
        let pos = if data.is_empty() {
            0
        } else {
            rng.gen_range(0..=data.len())
        };
        match rng.gen_range(0..5) {
            0 if pos < data.len() => data[pos] = rng.gen(),
            1 => {
                let tok = TOKENS[rng.gen_range(0..TOKENS.len())];
                data.splice(pos..pos, tok.iter().copied());
            }
            2 if pos < data.len() => {
                let end = (pos + rng.gen_range(1..20)).min(data.len());
                data.drain(pos..end);
            }
            3 => data.truncate(pos),
            _ => {}
        }
    }
    data
}

fn exercise_sip_text(msg: &str) {
    let _ = utils::parse_status_code(msg);
    let _ = utils::extract_auth_challenge(msg);
    let _ = utils::extract_all_auth_challenges(msg);
    let _ = utils::extract_to_tag(msg);
    let _ = utils::extract_quoted(msg, "realm");
    for name in [
        "Via",
        "v",
        "To",
        "From",
        "Contact",
        "Call-ID",
        "CSeq",
        "WWW-Authenticate",
    ] {
        let value = utils::extract_header(msg, name);
        let _ = utils::extract_uri(&value);
        let _ = utils::split_header_values(&value);
        let _ = utils::extract_param(msg, name, "tag");
    }
    let _ = utils::extract_headers_raw(msg, "Record-Route");
    let routes = utils::extract_record_routes(msg);
    let _ = utils::format_route_headers(&routes);
    let _ = utils::clean_uri(msg);
    let _ = utils::validate_header_value(msg, "fuzz");
    let _ = utils::parse_session_expires(msg);
    let _ = utils::parse_min_se(msg);
    let _ = utils::parse_rseq(msg);
    let _ = utils::parse_rack(msg);
    let _ = rsipclient::sip::transaction::build_prack_200_ok(msg);
    let parsed = sdp::parse_sdp(msg);
    let _ = parsed.rtp_addr();
    let _ = sdp::parse_remote_codecs(msg);
    let _ = rsipclient::sip::transport::parse_content_length(msg);
}

#[test]
fn sip_and_sdp_parsers_survive_mutated_messages() {
    let mut rng = StdRng::seed_from_u64(0x5151_2026);
    exercise_sip_text(SAMPLE_SIP);
    for _ in 0..ITERATIONS {
        let bytes = mutate(&mut rng, SAMPLE_SIP.as_bytes());
        exercise_sip_text(&String::from_utf8_lossy(&bytes));
    }
}

#[test]
fn stream_framing_survives_random_byte_streams() {
    let mut rng = StdRng::seed_from_u64(7);
    for _ in 0..ITERATIONS {
        let mut buf = mutate(&mut rng, SAMPLE_SIP.as_bytes());
        // Prefix keep-alives and append a second message, as TCP delivers them.
        buf.splice(0..0, b"\r\n\r\n".iter().copied());
        buf.extend_from_slice(SAMPLE_SIP.as_bytes());
        let mut guard = 0;
        while let Some(msg) = rsipclient::sip::transport::extract_sip_message(&mut buf) {
            assert!(!msg.is_empty());
            guard += 1;
            assert!(guard < 1000, "framing loop does not make progress");
        }
    }
}

#[test]
fn rtp_parser_survives_random_packets() {
    let mut rng = StdRng::seed_from_u64(11);
    let mut seed = vec![0x80u8, 0, 0, 1, 0, 0, 0, 160, 1, 2, 3, 4];
    seed.extend_from_slice(&[0xFF; 160]);
    for _ in 0..ITERATIONS * 3 {
        let mut packet = mutate(&mut rng, &seed);
        if rng.gen_bool(0.5) && !packet.is_empty() {
            // Force version 2 with random P/X/CC bits so the deeper paths run.
            packet[0] = 0x80 | (rng.gen::<u8>() & 0x3F);
        }
        if let Some(rtp) = rsipclient::rtp::receiver::parse_rtp(&packet) {
            assert!(rtp.payload.len() <= packet.len());
        }
    }
}

#[test]
fn wav_parser_survives_mutated_files() {
    let mut rng = StdRng::seed_from_u64(13);
    let dir = std::env::temp_dir().join(format!("rsip-fuzz-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("seed.wav");
    rsipclient::rtp::receiver::save_wav(&[100, -100, 2000, -2000], 8000, path.to_str().unwrap())
        .unwrap();
    let seed = std::fs::read(&path).unwrap();
    std::fs::remove_dir_all(&dir).ok();

    for _ in 0..ITERATIONS {
        let data = mutate(&mut rng, &seed);
        if let Ok((info, samples)) = rsipclient::rtp::wav::parse_wav(&data) {
            assert!(info.sample_rate > 0 && info.channels > 0);
            assert!(samples.len() <= data.len());
        }
    }
}

#[test]
fn g711_decoders_accept_any_payload() {
    let mut rng = StdRng::seed_from_u64(17);
    for _ in 0..200 {
        let payload: Vec<u8> = (0..rng.gen_range(0..400)).map(|_| rng.gen()).collect();
        for codec in [
            rsipclient::rtp::codec::Codec::Pcmu,
            rsipclient::rtp::codec::Codec::Pcma,
        ] {
            assert_eq!(codec.decode(&payload).unwrap().len(), payload.len());
        }
        // Garbage Opus must be an error, never a panic.
        let _ = rsipclient::rtp::codec::Codec::Opus.decode(&payload);
    }
}
