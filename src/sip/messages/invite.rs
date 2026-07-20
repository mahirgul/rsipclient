//! SIP INVITE, ACK, BYE, and CANCEL message builders
//!
//! Builds raw SIP call control request strings.

use crate::sip::auth;
use crate::sip::settings::SipSettings;

/// Build INVITE request with SDP body
pub fn build_invite(
    target_uri: &str,
    username: &str,
    domain: &str,
    local_addr: &str,
    local_tag: &str,
    branch: &str,
    call_id: &str,
    cseq: u32,
    sdp: &str,
    settings: &SipSettings,
    via_transport: &str,
) -> String {
    let sdp_len = sdp.len();
    let from = settings.format_from(username, domain);
    let extra = settings.extra_headers();
    let scheme = if via_transport.to_uppercase() == "TLS" {
        "sips"
    } else {
        "sip"
    };

    format!(
        "INVITE {} SIP/2.0\r\n\
         Via: SIP/2.0/{} {};branch={}\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <{}>\r\n\
         Call-ID: {}\r\n\
         CSeq: {} INVITE\r\n\
         Contact: <{}:{}@{}>\r\n\
         {}Content-Type: application/sdp\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        target_uri,
        via_transport.to_uppercase(),
        local_addr,
        branch,
        from,
        local_tag,
        target_uri,
        call_id,
        cseq,
        scheme,
        username,
        local_addr,
        extra,
        sdp_len,
        sdp
    )
}

/// Build INVITE with MD5 Digest authentication header (for 401/407 challenges)
pub fn build_invite_with_auth(
    target_uri: &str,
    username: &str,
    password: &str,
    domain: &str,
    local_addr: &str,
    local_tag: &str,
    branch: &str,
    call_id: &str,
    cseq: u32,
    sdp: &str,
    realm: &str,
    nonce: &str,
    settings: &SipSettings,
    via_transport: &str,
) -> String {
    let uri = target_uri.to_string();
    let response_digest = auth::compute_digest(username, password, realm, nonce, "INVITE", &uri);
    let sdp_len = sdp.len();
    let from = settings.format_from(username, domain);
    let extra = settings.extra_headers();
    let scheme = if via_transport.to_uppercase() == "TLS" {
        "sips"
    } else {
        "sip"
    };

    format!(
        "INVITE {} SIP/2.0\r\n\
         Via: SIP/2.0/{} {};branch={}\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <{}>\r\n\
         Call-ID: {}\r\n\
         CSeq: {} INVITE\r\n\
         Contact: <{}:{}@{}>\r\n\
         Authorization: Digest username=\"{}\", realm=\"{}\", nonce=\"{}\", uri=\"{}\", response=\"{}\", algorithm=MD5\r\n\
         {}Content-Type: application/sdp\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        target_uri,
        via_transport.to_uppercase(),
        local_addr,
        branch,
        from,
        local_tag,
        target_uri,
        call_id,
        cseq,
        scheme,
        username,
        local_addr,
        username,
        realm,
        nonce,
        uri,
        response_digest,
        extra,
        sdp_len,
        sdp,
    )
}

/// Build ACK request
pub fn build_ack(
    target_uri: &str,
    username: &str,
    domain: &str,
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

    format!(
        "ACK {} SIP/2.0\r\n\
         Via: SIP/2.0/{} {};branch={}\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <{}>;tag={}\r\n\
         Call-ID: {}\r\n\
         CSeq: {} ACK\r\n\
         Content-Length: 0\r\n\
         \r\n",
        target_uri,
        via_transport.to_uppercase(),
        local_addr,
        branch,
        from,
        local_tag,
        target_uri,
        remote_tag,
        call_id,
        cseq
    )
}

/// Build BYE request
pub fn build_bye(
    username: &str,
    domain: &str,
    remote_uri: &str,
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

    format!(
        "BYE {} SIP/2.0\r\n\
         Via: SIP/2.0/{} {};branch={}\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <{}>;tag={}\r\n\
         Call-ID: {}\r\n\
         CSeq: {} BYE\r\n\
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
        cseq
    )
}

/// Build CANCEL request with optional Reason header (RFC 3326)
pub fn build_cancel(
    username: &str,
    domain: &str,
    remote_uri: &str,
    local_addr: &str,
    local_tag: &str,
    call_id: &str,
    cseq: u32,
    branch: &str,
    settings: &SipSettings,
    via_transport: &str,
) -> String {
    let from = settings.format_from(username, domain);

    format!(
        "CANCEL {} SIP/2.0\r\n\
         Via: SIP/2.0/{} {};branch={}\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <{}>\r\n\
         Call-ID: {}\r\n\
         CSeq: {} CANCEL\r\n\
         Reason: Q.850;cause=16;text=\"Normal call clearing\"\r\n\
         Content-Length: 0\r\n\
         \r\n",
        remote_uri,
        via_transport.to_uppercase(),
        local_addr,
        branch,
        from,
        local_tag,
        remote_uri,
        call_id,
        cseq
    )
}
