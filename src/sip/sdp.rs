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
#[allow(dead_code)]
pub fn build_sdp_single(username: &str, local_ip: &str, rtp_port: u16, codec: Codec) -> String {
    build_sdp(username, local_ip, rtp_port, &[codec])
}

/// Default SDP: PCMU + PCMA + Opus (if enabled)
pub fn build_sdp_default(username: &str, local_ip: &str, rtp_port: u16) -> String {
    #[allow(unused_mut)]
    let mut codecs = vec![Codec::Pcmu, Codec::Pcma];

    #[cfg(feature = "opus")]
    codecs.push(Codec::Opus);

    build_sdp(username, local_ip, rtp_port, &codecs)
}
