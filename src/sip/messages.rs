//! SIP message builders - raw SIP request strings
//!
//! All builders now accept `SipSettings` for optional headers like
//! P-Asserted-Identity, P-Preferred-Identity, User-Agent, Session-Expires, etc.
//! They also accept `via_transport` to format the Via and Contact headers dynamically.

use crate::sip::auth;
use crate::sip::settings::SipSettings;

/// Build REGISTER request (without auth header)
pub fn build_register(
    username: &str,
    domain: &str,
    local_addr: &str,
    local_tag: &str,
    branch: &str,
    call_id: &str,
    cseq: u32,
    settings: &SipSettings,
    via_transport: &str,
) -> (String, String, u32) {
    let from = settings.format_from(username, domain);
    let extra = settings.extra_headers();
    let expiry = settings.register_expiry;
    let scheme = if via_transport.to_uppercase() == "TLS" {
        "sips"
    } else {
        "sip"
    };

    let msg = format!(
        "REGISTER sip:{} SIP/2.0\r\n\
         Via: SIP/2.0/{} {};branch={}\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <sip:{}@{}>\r\n\
         Call-ID: {}\r\n\
         CSeq: {} REGISTER\r\n\
         Contact: <{}:{}@{}>\r\n\
         Expires: {}\r\n\
         {}Content-Length: 0\r\n\
         \r\n",
        domain,
        via_transport.to_uppercase(),
        local_addr,
        branch,
        from,
        local_tag,
        username,
        domain,
        call_id,
        cseq,
        scheme,
        username,
        local_addr,
        expiry,
        extra,
    );
    (msg, call_id.to_string(), cseq)
}

/// Build REGISTER with MD5 Digest authentication header
pub fn build_register_with_auth(
    username: &str,
    password: &str,
    domain: &str,
    local_addr: &str,
    local_tag: &str,
    branch: &str,
    call_id: &str,
    cseq: u32,
    realm: &str,
    nonce: &str,
    settings: &SipSettings,
    via_transport: &str,
) -> String {
    let uri = format!("sip:{}", domain);
    let response_digest = auth::compute_digest(username, password, realm, nonce, "REGISTER", &uri);
    let from = settings.format_from(username, domain);
    let extra = settings.extra_headers();
    let expiry = settings.register_expiry;
    let scheme = if via_transport.to_uppercase() == "TLS" {
        "sips"
    } else {
        "sip"
    };

    format!(
        "REGISTER sip:{} SIP/2.0\r\n\
         Via: SIP/2.0/{} {};branch={}\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <sip:{}@{}>\r\n\
         Call-ID: {}\r\n\
         CSeq: {} REGISTER\r\n\
         Contact: <{}:{}@{}>\r\n\
         Authorization: Digest username=\"{}\", realm=\"{}\", nonce=\"{}\", uri=\"{}\", response=\"{}\", algorithm=MD5\r\n\
         Expires: {}\r\n\
         {}Content-Length: 0\r\n\
         \r\n",
        domain, via_transport.to_uppercase(), local_addr, branch,
        from, local_tag,
        username, domain,
        call_id,
        cseq,
        scheme, username, local_addr,
        username, realm, nonce, uri, response_digest,
        expiry,
        extra,
    )
}

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
        target_uri, via_transport.to_uppercase(), local_addr, branch,
        from, local_tag,
        target_uri,
        call_id, cseq,
        scheme, username, local_addr,
        username, realm, nonce, uri, response_digest,
        extra, sdp_len, sdp,
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
         Via: SIP/2.0/{} {};branch={}\r\n\
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
         Via: SIP/2.0/{} {};branch={}\r\n\
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
         Via: SIP/2.0/{} {};branch={}\r\n\
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
         Via: SIP/2.0/{} {};branch={}\r\n\
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rfc3326_reason_header() {
        let settings = SipSettings::default();
        let cancel = build_cancel(
            "alice",
            "example.com",
            "sip:bob@example.com",
            "192.168.1.10:5060",
            "tag-1",
            "callid-1",
            1,
            "branch-1",
            &settings,
            "udp",
        );
        assert!(cancel.contains("Reason: Q.850;cause=16;text=\"Normal call clearing\""));
    }

    #[test]
    fn test_rfc3262_prack_builder() {
        let settings = SipSettings::default();
        let prack = build_prack(
            "sip:bob@example.com",
            "alice",
            "example.com",
            "192.168.1.10:5060",
            "tag-1",
            "tag-remote",
            "callid-1",
            2,
            1001,
            1,
            "branch-1",
            &settings,
            "udp",
        );
        assert!(prack.contains("PRACK sip:bob@example.com SIP/2.0"));
        assert!(prack.contains("RAck: 1001 1 INVITE"));
    }

    #[test]
    fn test_rfc3428_message_builder() {
        let settings = SipSettings::default();
        let msg = build_message(
            "sip:bob@example.com",
            "alice",
            "example.com",
            "192.168.1.10:5060",
            "tag-1",
            "branch-1",
            "callid-1",
            1,
            "Hello World!",
            &settings,
            "udp",
        );
        assert!(msg.contains("MESSAGE sip:bob@example.com SIP/2.0"));
        assert!(msg.contains("Content-Type: text/plain;charset=UTF-8"));
        assert!(msg.contains("Hello World!"));
    }

    #[test]
    fn test_rfc6086_info_dtmf_builder() {
        let settings = SipSettings::default();
        let info = build_info_dtmf(
            "sip:bob@example.com",
            "alice",
            "example.com",
            "192.168.1.10:5060",
            "tag-1",
            "tag-remote",
            "callid-1",
            2,
            "branch-1",
            '5',
            250,
            &settings,
            "udp",
        );
        assert!(info.contains("INFO sip:bob@example.com SIP/2.0"));
        assert!(info.contains("Signal=5"));
        assert!(info.contains("Duration=250"));
    }
}
