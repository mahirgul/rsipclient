//! SIP REGISTER message builders
//!
//! Builds raw SIP REGISTER request strings (with and without MD5 Digest authentication).

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
        username,
        realm,
        nonce,
        uri,
        response_digest,
        expiry,
        extra,
    )
}
