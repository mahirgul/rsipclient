//! SIP call control — REFER (transfer), re-INVITE (hold/resume)

use crate::sip::settings::SipSettings;

/// Build a REFER request to transfer the call to a target
pub fn build_refer(
    username: &str,
    domain: &str,
    remote_uri: &str,
    refer_to: &str,
    local_addr: &str,
    local_tag: &str,
    remote_tag: &str,
    call_id: &str,
    cseq: u32,
    branch: &str,
    settings: &SipSettings,
    via_transport: &str,
) -> String {
    let from = settings.format_from(username, domain);
    let scheme = if via_transport.to_uppercase() == "TLS" {
        "sips"
    } else {
        "sip"
    };

    format!(
        "REFER {} SIP/2.0\r\n\
         Via: SIP/2.0/{} {};branch={}\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <{}>;tag={}\r\n\
         Call-ID: {}\r\n\
         CSeq: {} REFER\r\n\
         Contact: <{}:{}@{}>\r\n\
         Refer-To: <{}>\r\n\
         Content-Length: 0\r\n\
         \r\n",
        remote_uri,
        via_transport.to_uppercase(),
        local_addr,
        branch,
        from,
        local_tag,
        remote_uri,
        remote_tag,
        call_id,
        cseq,
        scheme,
        username,
        local_addr,
        refer_to,
    )
}

/// Build a re-INVITE to put a call on hold (sendonly/inactive)
pub fn build_hold(
    username: &str,
    domain: &str,
    remote_uri: &str,
    local_ip: &str,
    local_addr: &str,
    local_tag: &str,
    remote_tag: &str,
    call_id: &str,
    cseq: u32,
    branch: &str,
    rtp_port: u16,
    settings: &SipSettings,
    resume: bool,
    codec: &str,
    via_transport: &str,
) -> String {
    let from = settings.format_from(username, domain);
    let direction = if resume { "sendrecv" } else { "sendonly" };
    let scheme = if via_transport.to_uppercase() == "TLS" {
        "sips"
    } else {
        "sip"
    };

    // Build codec-specific SDP rtpmap lines
    let (payload_type, rtpmap_line) = match codec {
        "opus" => ("111", "a=rtpmap:111 opus/48000/2\r\n"),
        "pcma" | "g711a" | "alaw" => ("8", "a=rtpmap:8 PCMA/8000\r\n"),
        _ => ("0", "a=rtpmap:0 PCMU/8000\r\n"),
    };

    // SDP with configured codec, plus telephone-event
    let sdp = format!(
        "v=0\r\n\
         o={} 0 0 IN IP4 {}\r\n\
         s=hold\r\n\
         c=IN IP4 {}\r\n\
         t=0 0\r\n\
         m=audio {} RTP/AVP {} 101\r\n\
         {}a=rtpmap:101 telephone-event/8000\r\n\
         a={}\r\n",
        username, local_ip, local_ip, rtp_port, payload_type, rtpmap_line, direction
    );
    let sdp_len = sdp.len();

    format!(
        "INVITE {} SIP/2.0\r\n\
         Via: SIP/2.0/{} {};branch={}\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <{}>;tag={}\r\n\
         Call-ID: {}\r\n\
         CSeq: {} INVITE\r\n\
         Contact: <{}:{}@{}>\r\n\
         Content-Type: application/sdp\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        remote_uri,
        via_transport.to_uppercase(),
        local_addr,
        branch,
        from,
        local_tag,
        remote_uri,
        remote_tag,
        call_id,
        cseq,
        scheme,
        username,
        local_addr,
        sdp_len,
        sdp,
    )
}
