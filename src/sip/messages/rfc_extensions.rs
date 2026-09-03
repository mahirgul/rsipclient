//! SIP Extension Message Builders (RFC 3262, RFC 3428, RFC 6086, RFC 6665)
//!
//! Handles PRACK, MESSAGE, INFO DTMF, and SUBSCRIBE request construction.

use crate::sip::settings::SipSettings;

/// Build PRACK request for reliable provisional responses (RFC 3262)
#[allow(dead_code)]
pub fn build_prack(
    target_uri: &str,
    username: &str,
    domain: &str,
    local_addr: &str,
    local_tag: &str,
    remote_tag: &str,
    call_id: &str,
    cseq: u32,
    rseq: u32,
    invite_cseq: u32,
    branch: &str,
    settings: &SipSettings,
    via_transport: &str,
) -> String {
    let from = settings.format_from(username, domain);

    format!(
        "PRACK {} SIP/2.0\r\n\
         Via: SIP/2.0/{} {};branch={};rport\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <{}>;tag={}\r\n\
         Call-ID: {}\r\n\
         CSeq: {} PRACK\r\n\
         RAck: {} {} INVITE\r\n\
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
        cseq,
        rseq,
        invite_cseq
    )
}

/// Build SIP MESSAGE request for instant messaging (RFC 3428)
pub fn build_message(
    target_uri: &str,
    username: &str,
    domain: &str,
    local_addr: &str,
    local_tag: &str,
    branch: &str,
    call_id: &str,
    cseq: u32,
    text_body: &str,
    settings: &SipSettings,
    via_transport: &str,
) -> String {
    let from = settings.format_from(username, domain);
    let body_len = text_body.len();

    format!(
        "MESSAGE {} SIP/2.0\r\n\
         Via: SIP/2.0/{} {};branch={};rport\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <{}>\r\n\
         Call-ID: {}\r\n\
         CSeq: {} MESSAGE\r\n\
         Content-Type: text/plain;charset=UTF-8\r\n\
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
        body_len,
        text_body
    )
}

/// Build SIP INFO request for out-of-band DTMF relay (RFC 6086)
pub fn build_info_dtmf(
    target_uri: &str,
    username: &str,
    domain: &str,
    local_addr: &str,
    local_tag: &str,
    remote_tag: &str,
    call_id: &str,
    cseq: u32,
    branch: &str,
    digit: char,
    duration_ms: u32,
    settings: &SipSettings,
    via_transport: &str,
) -> String {
    let from = settings.format_from(username, domain);
    let body = format!("Signal={}\r\nDuration={}\r\n", digit, duration_ms);
    let body_len = body.len();

    format!(
        "INFO {} SIP/2.0\r\n\
         Via: SIP/2.0/{} {};branch={};rport\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <{}>;tag={}\r\n\
         Call-ID: {}\r\n\
         CSeq: {} INFO\r\n\
         Content-Type: application/dtmf-relay\r\n\
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
        remote_tag,
        call_id,
        cseq,
        body_len,
        body
    )
}

/// Build SIP SUBSCRIBE request for event notifications (RFC 6665 / RFC 3265)
#[allow(dead_code)]
pub fn build_subscribe(
    target_uri: &str,
    username: &str,
    domain: &str,
    local_addr: &str,
    local_tag: &str,
    branch: &str,
    call_id: &str,
    cseq: u32,
    event_type: &str,
    expires_secs: u32,
    settings: &SipSettings,
    via_transport: &str,
) -> String {
    let from = settings.format_from(username, domain);

    format!(
        "SUBSCRIBE {} SIP/2.0\r\n\
         Via: SIP/2.0/{} {};branch={};rport\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <{}>\r\n\
         Call-ID: {}\r\n\
         CSeq: {} SUBSCRIBE\r\n\
         Event: {}\r\n\
         Expires: {}\r\n\
         Content-Length: 0\r\n\
         \r\n",
        target_uri,
        via_transport.to_uppercase(),
        local_addr,
        branch,
        from,
        local_tag,
        target_uri,
        call_id,
        cseq,
        event_type,
        expires_secs
    )
}
