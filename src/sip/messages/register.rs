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

    let instance_uuid = {
        let digest = md5::compute(format!("{}:{}", username, domain));
        let hex = format!("{:x}", digest);
        format!(
            "{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        )
    };
    let contact = format!(
        "<{}:{}@{}>;reg-id=1;+sip.instance=\"<urn:uuid:{}>\"",
        scheme, username, local_addr, instance_uuid
    );

    let msg = format!(
        "REGISTER sip:{} SIP/2.0\r\n\
         Via: SIP/2.0/{} {};branch={};rport\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <sip:{}@{}>\r\n\
         Call-ID: {}\r\n\
         CSeq: {} REGISTER\r\n\
         Contact: {}\r\n\
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
    challenge: &crate::sip::utils::AuthChallenge,
    settings: &SipSettings,
    via_transport: &str,
) -> String {
    let uri = format!("sip:{}", domain);
    let auth_header =
        auth::build_authorization_header(username, password, challenge, "REGISTER", &uri);
    let from = settings.format_from(username, domain);
    let extra = settings.extra_headers();
    let expiry = settings.register_expiry;
    let scheme = if via_transport.to_uppercase() == "TLS" {
        "sips"
    } else {
        "sip"
    };

    let instance_uuid = {
        let digest = md5::compute(format!("{}:{}", username, domain));
        let hex = format!("{:x}", digest);
        format!(
            "{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        )
    };
    let contact = format!(
        "<{}:{}@{}>;reg-id=1;+sip.instance=\"<urn:uuid:{}>\"",
        scheme, username, local_addr, instance_uuid
    );

    format!(
        "REGISTER sip:{} SIP/2.0\r\n\
         Via: SIP/2.0/{} {};branch={};rport\r\n\
         Max-Forwards: 70\r\n\
         From: {};tag={}\r\n\
         To: <sip:{}@{}>\r\n\
         Call-ID: {}\r\n\
         CSeq: {} REGISTER\r\n\
         Contact: {}\r\n\
         {}\r\n\
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
        auth_header,
        expiry,
        extra,
    )
}
