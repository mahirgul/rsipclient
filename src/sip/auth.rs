//! Digest authentication (MD5) for SIP

use md5;

/// Compute MD5 Digest response per RFC 2617:
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
}
