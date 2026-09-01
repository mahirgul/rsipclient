//! SDP (Session Description Protocol) body builder for SIP INVITE

use crate::rtp::codec::Codec;

/// Build an SDP body advertising all supported codecs
pub fn build_sdp(username: &str, local_ip: &str, rtp_port: u16, codecs: &[Codec]) -> String {
    // Build payload type list: "0 8 111 101"
    let pt_list: Vec<String> = codecs
        .iter()
        .map(|c| c.payload_type().to_string())
        .collect();
    let pt_str = pt_list.join(" ");

    // Build rtpmap lines
    let mut rtpmap_lines = String::new();
    for codec in codecs {
        rtpmap_lines.push_str(&format!("a=rtpmap:{}\r\n", codec.rtpmap()));
    }
    // Always add telephone-event
    rtpmap_lines.push_str("a=rtpmap:101 telephone-event/8000\r\n");

    // Add fmtp for Opus
    let fmtp_line = if codecs.contains(&Codec::Opus) {
        "a=fmtp:111 minptime=10;useinbandfec=1\r\n"
    } else {
        ""
    };

    format!(
        "v=0\r\n\
         o={user} 0 0 IN IP4 {ip}\r\n\
         s=rust-sip-client\r\n\
         c=IN IP4 {ip}\r\n\
         t=0 0\r\n\
         m=audio {port} RTP/AVP {pts} 101\r\n\
         {rtpmaps}\
         {fmtp}\
         a=sendrecv\r\n",
        user = username,
        ip = local_ip,
        port = rtp_port,
        pts = pt_str,
        rtpmaps = rtpmap_lines,
        fmtp = fmtp_line,
    )
}

/// Build a minimal SDP with just one codec
pub fn build_sdp_single(username: &str, local_ip: &str, rtp_port: u16, codec: Codec) -> String {
    build_sdp(username, local_ip, rtp_port, &[codec])
}

/// Parse the list of audio codecs advertised in a remote SDP body.
///
/// Reads the `m=audio` payload-type list and resolves each payload type to a
/// codec, first via static/dynamic payload-type assignments, then via the
/// `a=rtpmap` encoding name.
pub fn parse_remote_codecs(msg: &str) -> Vec<Codec> {
    let mut codecs = Vec::new();

    // Collect dynamic payload-type → encoding mappings from a=rtpmap lines.
    let mut pt_names: std::collections::HashMap<u8, String> = std::collections::HashMap::new();
    for line in msg.lines() {
        if let Some(rest) = line.strip_prefix("a=rtpmap:") {
            let mut parts = rest.split_whitespace();
            if let (Some(pt_str), Some(enc)) = (parts.next(), parts.next()) {
                if let Ok(pt) = pt_str.parse::<u8>() {
                    let encoding = enc.split('/').next().unwrap_or("").to_ascii_lowercase();
                    pt_names.insert(pt, encoding);
                }
            }
        }
    }

    if let Some(m_line) = msg.lines().find(|l| l.starts_with("m=audio")) {
        let parts: Vec<&str> = m_line.split_whitespace().collect();
        // m=audio <port> <proto> <pt> [<pt> ...]
        for pt_str in parts.iter().skip(3) {
            let Ok(pt) = pt_str.parse::<u8>() else {
                continue;
            };
            if let Some(codec) = Codec::from_payload_type(pt) {
                codecs.push(codec);
            } else if let Some(enc) = pt_names.get(&pt) {
                match enc.as_str() {
                    "pcmu" => codecs.push(Codec::Pcmu),
                    "pcma" => codecs.push(Codec::Pcma),
                    "opus" => codecs.push(Codec::Opus),
                    _ => {}
                }
            }
        }
    }

    codecs.dedup();
    codecs
}

/// Log a warning if the remote SDP does not support our configured codec.
pub fn warn_codec_mismatch(configured: Codec, msg: &str) {
    let remote = parse_remote_codecs(msg);
    if remote.is_empty() {
        return;
    }
    if !remote.contains(&configured) {
        log::warn!(
            "Remote does not support configured codec {:?}; remote offers {:?}",
            configured,
            remote
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_remote_codecs_reads_static_and_dynamic() {
        let sdp = "v=0\r\n\
                   o=x 0 0 IN IP4 1.2.3.4\r\n\
                   c=IN IP4 1.2.3.4\r\n\
                   m=audio 8000 RTP/AVP 0 111 101\r\n\
                   a=rtpmap:111 opus/48000/2\r\n\
                   a=rtpmap:101 telephone-event/8000\r\n";
        let codecs = parse_remote_codecs(sdp);
        assert!(codecs.contains(&Codec::Pcmu));
        assert!(codecs.contains(&Codec::Opus));
        assert!(!codecs.contains(&Codec::Pcma));
    }

    #[test]
    fn parse_remote_codecs_uses_rtpmap_for_nonstandard_pt() {
        let sdp = "m=audio 8000 RTP/AVP 96 101\r\n\
                   a=rtpmap:96 PCMA/8000\r\n";
        let codecs = parse_remote_codecs(sdp);
        assert_eq!(codecs, vec![Codec::Pcma]);
    }
}
