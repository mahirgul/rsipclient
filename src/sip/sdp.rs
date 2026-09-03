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

/// Media stream direction (RFC 4566)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MediaDirection {
    #[default]
    SendRecv,
    SendOnly,
    RecvOnly,
    Inactive,
}

/// Parsed SDP description (RFC 4566, RFC 3605, RFC 4568)
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ParsedSdp {
    /// Remote media IP address (from media-level `c=` or session-level `c=`)
    pub media_ip: Option<String>,
    /// Remote media RTP port (from `m=audio <port>`)
    pub media_port: Option<u16>,
    /// Transport protocol (e.g. "RTP/AVP", "RTP/SAVP")
    pub proto: Option<String>,
    /// Remote RTCP port (from `a=rtcp:<port>`, RFC 3605)
    pub rtcp_port: Option<u16>,
    /// Media direction (sendrecv, sendonly, recvonly, inactive)
    pub direction: MediaDirection,
    /// Offered codecs resolved from payload types and a=rtpmap
    pub codecs: Vec<Codec>,
    /// SRTP crypto attributes (RFC 4568 SDES)
    pub crypto_suites: Vec<SrtpCrypto>,
}

impl ParsedSdp {
    /// Combined remote RTP SocketAddr if IP and port are present
    pub fn rtp_addr(&self) -> Option<std::net::SocketAddr> {
        let ip = self.media_ip.as_ref()?;
        let port = self.media_port?;
        format!("{}:{}", ip, port).parse().ok()
    }
}

/// RFC 4568 SDES SRTP crypto suite
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SrtpCrypto {
    pub tag: u32,
    pub suite: String,
    pub key_salt: String,
}

impl SrtpCrypto {
    /// Format as SDP attribute line: `a=crypto:1 AES_CM_128_HMAC_SHA1_80 inline:...`
    #[allow(dead_code)]
    pub fn to_sdp_attribute(&self) -> String {
        format!(
            "a=crypto:{} {} inline:{}\r\n",
            self.tag, self.suite, self.key_salt
        )
    }

    /// Parse an `a=crypto:` line
    pub fn parse(line: &str) -> Option<Self> {
        let rest = line.strip_prefix("a=crypto:")?.trim();
        let mut parts = rest.split_whitespace();
        let tag = parts.next()?.parse::<u32>().ok()?;
        let suite = parts.next()?.to_string();
        let inline = parts.next()?;
        let key_salt = inline.strip_prefix("inline:")?.to_string();
        Some(Self {
            tag,
            suite,
            key_salt,
        })
    }
}

/// Parse full SDP from SIP body or message (RFC 4566, RFC 3605, RFC 4568).
pub fn parse_sdp(msg: &str) -> ParsedSdp {
    let mut session_ip: Option<String> = None;
    let mut media_ip: Option<String> = None;
    let mut media_port: Option<u16> = None;
    let mut proto: Option<String> = None;
    let mut rtcp_port: Option<u16> = None;
    let mut direction = MediaDirection::SendRecv;
    let mut crypto_suites = Vec::new();
    let mut in_audio_media = false;

    for line in msg.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("c=") {
            // c=IN IP4 192.168.1.1
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if let Some(&ip) = parts.get(2) {
                if in_audio_media {
                    media_ip = Some(ip.to_string());
                } else {
                    session_ip = Some(ip.to_string());
                }
            }
        } else if let Some(rest) = trimmed.strip_prefix("m=") {
            if rest.starts_with("audio") {
                in_audio_media = true;
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if let Some(p_str) = parts.get(1) {
                    media_port = p_str.parse::<u16>().ok();
                }
                if let Some(&pr) = parts.get(2) {
                    proto = Some(pr.to_string());
                }
            } else {
                in_audio_media = false;
            }
        } else if in_audio_media {
            if trimmed == "a=sendrecv" {
                direction = MediaDirection::SendRecv;
            } else if trimmed == "a=sendonly" {
                direction = MediaDirection::SendOnly;
            } else if trimmed == "a=recvonly" {
                direction = MediaDirection::RecvOnly;
            } else if trimmed == "a=inactive" {
                direction = MediaDirection::Inactive;
            } else if let Some(rest) = trimmed.strip_prefix("a=rtcp:") {
                // a=rtcp:9000 or a=rtcp:9000 IN IP4 ...
                if let Some(port_str) = rest.split_whitespace().next() {
                    rtcp_port = port_str.parse::<u16>().ok();
                }
            } else if let Some(crypto) = SrtpCrypto::parse(trimmed) {
                crypto_suites.push(crypto);
            }
        }
    }

    let codecs = parse_remote_codecs(msg);
    let resolved_ip = media_ip.or(session_ip);

    ParsedSdp {
        media_ip: resolved_ip,
        media_port,
        proto,
        rtcp_port,
        direction,
        codecs,
        crypto_suites,
    }
}

/// Parse remote RTP address from SDP (RFC 4566).
pub fn parse_sdp_connection(msg: &str) -> Option<std::net::SocketAddr> {
    parse_sdp(msg).rtp_addr()
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

    #[test]
    fn test_parse_sdp_extended() {
        let sdp = "v=0\r\n\
                   o=alice 100 200 IN IP4 10.0.0.1\r\n\
                   s=SIP Call\r\n\
                   c=IN IP4 10.0.0.1\r\n\
                   m=audio 16384 RTP/SAVP 0 101\r\n\
                   c=IN IP4 192.168.1.50\r\n\
                   a=rtcp:16385\r\n\
                   a=sendonly\r\n\
                   a=crypto:1 AES_CM_128_HMAC_SHA1_80 inline:W3UZsDQrOZfZBsdfjasd834hdj284jsdflkasjdf\r\n";

        let parsed = parse_sdp(sdp);
        // Media-level c= must override session-level c=
        assert_eq!(parsed.media_ip.as_deref(), Some("192.168.1.50"));
        assert_eq!(parsed.media_port, Some(16384));
        assert_eq!(parsed.proto.as_deref(), Some("RTP/SAVP"));
        assert_eq!(parsed.rtcp_port, Some(16385));
        assert_eq!(parsed.direction, MediaDirection::SendOnly);
        assert_eq!(parsed.crypto_suites.len(), 1);
        assert_eq!(parsed.crypto_suites[0].tag, 1);
        assert_eq!(parsed.crypto_suites[0].suite, "AES_CM_128_HMAC_SHA1_80");

        let addr = parsed.rtp_addr().expect("valid socket addr");
        assert_eq!(addr.to_string(), "192.168.1.50:16384");
    }
}
