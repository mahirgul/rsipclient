//! Digest authentication (MD5) for SIP

use crate::sip::utils::AuthChallenge;
use md5;
use uuid::Uuid;

/// Compute MD5 Digest response per RFC 2617 (no qop):
/// MD5(MD5(username:realm:password) : nonce : MD5(method:uri))
pub fn compute_digest(
    username: &str,
    password: &str,
    realm: &str,
    nonce: &str,
    method: &str,
    uri: &str,
) -> String {
    let ha1_input = format!("{}:{}:{}", username, realm, password);
    let ha1 = format!("{:x}", md5::compute(ha1_input));

    let ha2_input = format!("{}:{}", method, uri);
    let ha2 = format!("{:x}", md5::compute(ha2_input));

    let response_input = format!("{}:{}:{}", ha1, nonce, ha2);
    format!("{:x}", md5::compute(response_input))
}

/// Compute MD5 Digest response with `qop="auth"` per RFC 2617 §3.2.2:
/// response = MD5( HA1 : nonce : nc : cnonce : qop : HA2 )
pub fn compute_digest_qop(
    username: &str,
    password: &str,
    realm: &str,
    nonce: &str,
    method: &str,
    uri: &str,
    cnonce: &str,
    nc: &str,
) -> String {
    let ha1 = format!(
        "{:x}",
        md5::compute(format!("{}:{}:{}", username, realm, password))
    );
    let ha2 = format!("{:x}", md5::compute(format!("{}:{}", method, uri)));
    let response_input = format!("{}:{}:{}:{}:auth:{}", ha1, nonce, nc, cnonce, ha2);
    format!("{:x}", md5::compute(response_input))
}

/// Generate a random client nonce (cnonce) as 32 lowercase hex chars.
pub fn generate_cnonce() -> String {
    Uuid::new_v4().simple().to_string()
}

/// Build the full `Authorization: Digest ...` header value for a challenge,
/// using `qop="auth"` when the server offers it.
pub fn build_authorization_header(
    username: &str,
    password: &str,
    challenge: &AuthChallenge,
    method: &str,
    uri: &str,
) -> String {
    let cnonce = generate_cnonce();
    let nc = "00000001";

    let (response, qop_part) = if challenge.supports_qop_auth() {
        let r = compute_digest_qop(
            username,
            password,
            &challenge.realm,
            &challenge.nonce,
            method,
            uri,
            &cnonce,
            nc,
        );
        (r, format!("qop=auth, cnonce=\"{}\", nc={}", cnonce, nc))
    } else {
        let r = compute_digest(
            username,
            password,
            &challenge.realm,
            &challenge.nonce,
            method,
            uri,
        );
        (r, String::new())
    };

    let mut header = format!(
        "Authorization: Digest username=\"{}\", realm=\"{}\", nonce=\"{}\", uri=\"{}\", response=\"{}\", algorithm=MD5",
        username, challenge.realm, challenge.nonce, uri, response
    );
    if !qop_part.is_empty() {
        header.push_str(&format!(", {}", qop_part));
    }
    if let Some(ref opaque) = challenge.opaque {
        header.push_str(&format!(", opaque=\"{}\"", opaque));
    }
    header
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_digest() {
        let result = compute_digest(
            "Mufasa",
            "Circle of Life",
            "testrealm@host.com",
            "dcd98b7102dd2f0e8b11d0f600bfb0c093",
            "REGISTER",
            "sip:test.example.com",
        );
        assert!(!result.is_empty());
    }

    /// RFC 2617 §3.5 worked example with qop="auth".
    #[test]
    fn test_compute_digest_qop_rfc2617_vector() {
        let result = compute_digest_qop(
            "Mufasa",
            "Circle Of Life",
            "testrealm@host.com",
            "dcd98b7102dd2f0e8b11d0f600bfb0c093",
            "GET",
            "/dir/index.html",
            "0a4f113b",
            "00000001",
        );
        assert_eq!(result, "6629fae49393a05397450978507c4ef1");
    }
}
