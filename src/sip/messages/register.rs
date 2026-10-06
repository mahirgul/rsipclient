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
    let msg = register_request(
        username,
        domain,
        local_addr,
        local_tag,
        branch,
        call_id,
        cseq,
        None,
        settings,
        via_transport,
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
    challenge: &crate::sip::utils::AuthChallenge,
    settings: &SipSettings,
    via_transport: &str,
) -> String {
    let uri = format!("sip:{}", domain);
    let auth_header =
        auth::build_authorization_header(username, password, challenge, "REGISTER", &uri);
    register_request(
        username,
        domain,
        local_addr,
        local_tag,
        branch,
        call_id,
        cseq,
        Some(&auth_header),
        settings,
        via_transport,
    )
}

/// Stable RFC 5626 instance ID for this user, formatted as a UUID.
///
/// Derived from the address of record so that re-registrations (and
/// restarts) present the same instance to the registrar.
fn instance_uuid(username: &str, domain: &str) -> String {
    let hex = format!("{:x}", md5::compute(format!("{}:{}", username, domain)));
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Shared REGISTER builder; `auth_header` is a full `Authorization:` or
/// `Proxy-Authorization:` line without the trailing CRLF.
fn register_request(
    username: &str,
    domain: &str,
    local_addr: &str,
    local_tag: &str,
    branch: &str,
    call_id: &str,
    cseq: u32,
    auth_header: Option<&str>,
    settings: &SipSettings,
    via_transport: &str,
) -> String {
    let from = settings.format_from(username, domain);
    let extra = settings.extra_headers();
    let scheme = if via_transport.eq_ignore_ascii_case("TLS") {
        "sips"
    } else {
        "sip"
    };
    let contact = format!(
        "<{}:{}@{}>;reg-id=1;+sip.instance=\"<urn:uuid:{}>\"",
        scheme,
        username,
        local_addr,
        instance_uuid(username, domain)
    );
    let auth_line = auth_header
        .map(|h| format!("{}\r\n", h))
        .unwrap_or_default();

    format!(
        "REGISTER sip:{} SIP/2.0\r\n\
         Via: SIP/2.0/{} {};branch={};rport\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <sip:{}@{}>\r\n\
         Call-ID: {}\r\n\
         CSeq: {} REGISTER\r\n\
         Contact: {}\r\n\
         {}\
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
        contact,
        auth_line,
        settings.register_expiry,
        extra,
    )
}
