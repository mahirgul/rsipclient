//! SIP message builders - raw SIP request strings
//!
//! All builders now accept `SipSettings` for optional headers like
//! P-Asserted-Identity, P-Preferred-Identity, User-Agent, Session-Expires, etc.

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
) -> (String, String, u32) {
    let from = settings.format_from(username, domain);
    let extra = settings.extra_headers();
    let expiry = settings.register_expiry;

    let msg = format!(
        "REGISTER sip:{} SIP/2.0\r\n\
         Via: SIP/2.0/UDP {};branch={}\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <sip:{}@{}>\r\n\
         Call-ID: {}\r\n\
         CSeq: {} REGISTER\r\n\
         Contact: <sip:{}@{}>\r\n\
         Expires: {}\r\n\
         {}Content-Length: 0\r\n\
         \r\n",
        domain,
        local_addr,
        branch,
        from,
        local_tag,
        username,
        domain,
        call_id,
        cseq,
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
) -> String {
    let uri = format!("sip:{}", domain);
    let response_digest = auth::compute_digest(username, password, realm, nonce, "REGISTER", &uri);
    let from = settings.format_from(username, domain);
    let extra = settings.extra_headers();
    let expiry = settings.register_expiry;

    format!(
        "REGISTER sip:{} SIP/2.0\r\n\
         Via: SIP/2.0/UDP {};branch={}\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <sip:{}@{}>\r\n\
         Call-ID: {}\r\n\
         CSeq: {} REGISTER\r\n\
         Contact: <sip:{}@{}>\r\n\
         Authorization: Digest username=\"{}\", realm=\"{}\", nonce=\"{}\", uri=\"{}\", response=\"{}\", algorithm=MD5\r\n\
         Expires: {}\r\n\
         {}Content-Length: 0\r\n\
         \r\n",
        domain, local_addr, branch,
        from, local_tag,
        username, domain,
        call_id,
        cseq,
        username, local_addr,
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
) -> String {
    let sdp_len = sdp.len();
    let from = settings.format_from(username, domain);
    let extra = settings.extra_headers();

    format!(
        "INVITE {} SIP/2.0\r\n\
         Via: SIP/2.0/UDP {};branch={}\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <{}>\r\n\
         Call-ID: {}\r\n\
         CSeq: {} INVITE\r\n\
         Contact: <sip:{}@{}>\r\n\
         {}Content-Type: application/sdp\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        target_uri,
        local_addr,
        branch,
        from,
        local_tag,
        target_uri,
        call_id,
        cseq,
        username,
        local_addr,
        extra,
        sdp_len,
        sdp
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
) -> String {
    let from = settings.format_from(username, domain);

    format!(
        "ACK {} SIP/2.0\r\n\
         Via: SIP/2.0/UDP {};branch={}\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <{}>;tag={}\r\n\
         Call-ID: {}\r\n\
         CSeq: {} ACK\r\n\
         Content-Length: 0\r\n\
         \r\n",
        target_uri, local_addr, branch, from, local_tag, target_uri, remote_tag, call_id, cseq
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
) -> String {
    let from = settings.format_from(username, domain);

    format!(
        "BYE {} SIP/2.0\r\n\
         Via: SIP/2.0/UDP {};branch={}\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <{}>;tag={}\r\n\
         Call-ID: {}\r\n\
         CSeq: {} BYE\r\n\
         Content-Length: 0\r\n\
         \r\n",
        remote_uri, local_addr, branch, from, local_tag, remote_uri, remote_tag, call_id, cseq
    )
}

/// Build CANCEL request
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
) -> String {
    let from = settings.format_from(username, domain);

    format!(
        "CANCEL {} SIP/2.0\r\n\
         Via: SIP/2.0/UDP {};branch={}\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <{}>\r\n\
         Call-ID: {}\r\n\
         CSeq: {} CANCEL\r\n\
         Content-Length: 0\r\n\
         \r\n",
        remote_uri, local_addr, branch, from, local_tag, remote_uri, call_id, cseq
    )
}
