//! SIP utility functions: parsing status codes, extracting headers, ID generation
//!
//! Parsing note: SIP header names and parameter keys are ASCII, but header
//! *values* carry arbitrary UTF-8 from the network (display names, realms,
//! tags). Matching is therefore done ASCII-case-insensitively on the original
//! string — never against a `to_lowercase()` copy, whose byte offsets can
//! drift from the original (e.g. 'İ' is 2 bytes but lowercases to 3) and would
//! slice mid-character and panic.

use anyhow::{Context, Result};
use uuid::Uuid;

/// True if `line` starts with `prefix`, compared ASCII-case-insensitively.
fn starts_with_ascii_ci(line: &str, prefix: &str) -> bool {
    let (bytes, pfx) = (line.as_bytes(), prefix.as_bytes());
    bytes.len() >= pfx.len() && bytes[..pfx.len()].eq_ignore_ascii_case(pfx)
}

/// True if `b` separates SIP parameters (`;tag=`, `, nonce=`, ` realm=`).
fn is_param_separator(b: u8) -> bool {
    matches!(b, b';' | b',' | b' ' | b'\t')
}

/// Parse SIP status code from first line (e.g. "SIP/2.0 200 OK" → 200)
pub fn parse_status_code(response: &str) -> Result<u16> {
    let first = response.lines().next().context("Empty response")?;
    let parts: Vec<&str> = first.split_whitespace().collect();
    if parts.len() >= 2 {
        parts[1].parse().context("Invalid status code")
    } else {
        anyhow::bail!("Cannot parse status line: {}", first)
    }
}

/// Parsed Digest authentication challenge (RFC 2617 §3.2.1).
#[derive(Debug, Clone, Default)]
pub struct AuthChallenge {
    pub realm: String,
    pub nonce: String,
    pub opaque: Option<String>,
    /// Comma-separated list of qop options as sent by the server, e.g. "auth,auth-int".
    pub qop: Option<String>,
    /// Parsed but not yet acted on (reserved for MD5-sess / stale-nonce retry).
    #[allow(dead_code)]
    pub algorithm: Option<String>,
    /// Set when the server says the nonce expired; the same credentials retried
    /// against the fresh nonce are expected to succeed.
    pub stale: bool,
    /// True when the challenge arrived in Proxy-Authenticate. RFC 3261 §22.3
    /// requires answering it with Proxy-Authorization, not Authorization —
    /// a proxy that gets the wrong header challenges again and the call or
    /// registration never completes.
    pub proxy: bool,
}

impl AuthChallenge {
    /// True if the server offers the `auth` quality-of-protection.
    pub fn supports_qop_auth(&self) -> bool {
        self.qop
            .as_ref()
            .map(|q| q.split(',').any(|s| s.trim().eq_ignore_ascii_case("auth")))
            .unwrap_or(false)
    }
}

/// Parse one challenge header line. Returns None for a header this client
/// cannot answer, such as a Basic challenge carrying no nonce.
fn parse_challenge_line(header: &str, proxy: bool) -> Option<AuthChallenge> {
    Some(AuthChallenge {
        realm: extract_quoted(header, "realm=")?,
        nonce: extract_quoted(header, "nonce=")?,
        opaque: extract_quoted(header, "opaque="),
        qop: extract_quoted(header, "qop="),
        algorithm: extract_quoted(header, "algorithm="),
        stale: extract_quoted(header, "stale=")
            .map(|s| s.eq_ignore_ascii_case("true"))
            .unwrap_or(false),
        proxy,
    })
}

/// Parse a Digest challenge from WWW-Authenticate / Proxy-Authenticate.
///
/// A response can carry several challenges — a proxy and the registrar each
/// add their own, and a server may offer a scheme this client cannot answer.
/// Prefer the one matching the status code (407 is the proxy's), and skip
/// headers that do not parse as a Digest challenge instead of giving up on the
/// first one.
pub fn extract_auth_challenge(response: &str) -> Option<AuthChallenge> {
    let prefer_proxy = parse_status_code(response)
        .map(|s| s == 407)
        .unwrap_or(false);
    let mut fallback: Option<AuthChallenge> = None;

    for line in response.lines() {
        let proxy = if starts_with_ascii_ci(line, "proxy-authenticate:") {
            true
        } else if starts_with_ascii_ci(line, "www-authenticate:") {
            false
        } else {
            continue;
        };

        let Some(challenge) = parse_challenge_line(line, proxy) else {
            continue;
        };

        let is_supported_algo = challenge
            .algorithm
            .as_deref()
            .map(|a| a.eq_ignore_ascii_case("md5"))
            .unwrap_or(true);

        if proxy == prefer_proxy && is_supported_algo {
            return Some(challenge);
        }

        let fallback_supported = fallback
            .as_ref()
            .and_then(|f| f.algorithm.as_deref())
            .map(|a| a.eq_ignore_ascii_case("md5"))
            .unwrap_or(true);

        if fallback.is_none() || (!fallback_supported && is_supported_algo) {
            fallback = Some(challenge);
        }
    }

    fallback
}

/// Extract all Digest authentication challenges from WWW-Authenticate and Proxy-Authenticate headers.
/// Sorted so that the preferred challenge (Proxy for 407, WWW for 401) appears first.
pub fn extract_all_auth_challenges(response: &str) -> Vec<AuthChallenge> {
    let prefer_proxy = parse_status_code(response)
        .map(|s| s == 407)
        .unwrap_or(false);
    let mut list = Vec::new();

    for line in response.lines() {
        let proxy = if starts_with_ascii_ci(line, "proxy-authenticate:") {
            true
        } else if starts_with_ascii_ci(line, "www-authenticate:") {
            false
        } else {
            continue;
        };

        let Some(challenge) = parse_challenge_line(line, proxy) else {
            continue;
        };

        let is_supported_algo = challenge
            .algorithm
            .as_deref()
            .map(|a| a.eq_ignore_ascii_case("md5"))
            .unwrap_or(true);

        if is_supported_algo {
            list.push(challenge);
        }
    }

    list.sort_by_key(|c| c.proxy != prefer_proxy);
    list
}

/// Match a SIP header line against a full header name or its compact alias.
/// Returns the length of the matching prefix **in `line`**, else None.
fn match_header_prefix(line: &str, name: &str) -> Option<usize> {
    let bytes = line.as_bytes();

    // Check full name — header names are ASCII, so the prefix length in the
    // original line is exactly `name.len() + 1` (for the colon).
    if bytes.len() > name.len()
        && bytes[..name.len()].eq_ignore_ascii_case(name.as_bytes())
        && bytes[name.len()] == b':'
    {
        return Some(name.len() + 1);
    }

    // Check compact name alias (RFC 3261 Section 7.3.3)
    let compact: Option<u8> = match name.to_ascii_lowercase().as_str() {
        "call-id" => Some(b'i'),
        "from" => Some(b'f'),
        "to" => Some(b't'),
        "via" => Some(b'v'),
        "contact" => Some(b'm'),
        "content-type" => Some(b'c'),
        "content-length" => Some(b'l'),
        "subject" => Some(b's'),
        "supported" => Some(b'k'),
        "content-encoding" => Some(b'e'),
        "accept-encoding" => Some(b'a'),
        _ => None,
    };

    if let Some(c) = compact {
        if bytes.len() > 1 && bytes[0].eq_ignore_ascii_case(&c) && bytes[1] == b':' {
            return Some(2);
        }
    }

    None
}

/// Extract the `tag` parameter from the To header.
pub fn extract_to_tag(response: &str) -> Option<String> {
    let to_line = response
        .lines()
        .find(|l| match_header_prefix(l, "to").is_some())?;
    extract_quoted(to_line, "tag=")
}

/// Find `key` in `line` at a parameter boundary, ASCII-case-insensitively.
///
/// Returns the byte offset just past the key. The key is required to sit at the
/// start of the line or directly after a parameter separator, so a `tag=` inside
/// a display name (`From: "tag=x" <sip:a@b>;tag=real`) cannot shadow the real
/// parameter. Keys are ASCII, so every returned offset is a char boundary.
fn find_param_value_start(line: &str, key: &str) -> Option<usize> {
    let (bytes, key_bytes) = (line.as_bytes(), key.as_bytes());
    if key_bytes.is_empty() || key_bytes.len() > bytes.len() {
        return None;
    }

    for start in 0..=bytes.len() - key_bytes.len() {
        if start > 0 && !is_param_separator(bytes[start - 1]) {
            continue;
        }
        if bytes[start..start + key_bytes.len()].eq_ignore_ascii_case(key_bytes) {
            return Some(start + key_bytes.len());
        }
    }
    None
}

/// Extract a quoted (or unquoted) parameter value from a SIP header line.
///
/// Supports:
///   `realm="sip.example.com"`  (quoted)
///   `expires=3600`             (unquoted)
pub fn extract_quoted(line: &str, key: &str) -> Option<String> {
    let start = find_param_value_start(line, key)?;
    let rest = line[start..].trim_start();

    if let Some(inner) = rest.strip_prefix('"') {
        let end = inner.find('"')?;
        Some(inner[..end].to_string())
    } else {
        let end = rest.find([',', ';', ' ', '\r', '\n']).unwrap_or(rest.len());
        Some(rest[..end].to_string())
    }
}

/// Extract the value of a SIP header line (minus the header name).
/// e.g. `extract_header(msg, "Call-ID")` → "abc123@sip.example.com"
pub fn extract_header(msg: &str, header_name: &str) -> String {
    for line in msg.lines() {
        if let Some(prefix_len) = match_header_prefix(line, header_name) {
            return line[prefix_len..].trim().to_string();
        }
    }
    String::new()
}

/// Extract a named parameter from a SIP header line.
/// e.g. `extract_param(msg, "From", "tag")` → "abc123"
pub fn extract_param(msg: &str, header_name: &str, param: &str) -> String {
    for line in msg.lines() {
        if match_header_prefix(line, header_name).is_some() {
            return extract_quoted(line, &format!("{}=", param)).unwrap_or_default();
        }
    }
    String::new()
}

/// Generate a short random ID with a prefix (e.g. "tag-a1b2c3d4")
pub fn short_id(prefix: &str) -> String {
    format!(
        "{}{}",
        prefix,
        Uuid::new_v4().to_string().split('-').next().unwrap()
    )
}

/// Extract SIP URI from a header value (e.g. `From: "Alice" <sip:alice@example.com>;tag=123` -> `sip:alice@example.com`)
pub fn extract_uri(header: &str) -> Option<String> {
    if let Some(start) = header.find('<') {
        if let Some(end) = header[start..].find('>') {
            return Some(header[start + 1..start + end].trim().to_string());
        }
    }
    // Fallback: strip after semicolon if no brackets
    let val = if let Some(idx) = header.find(';') {
        &header[..idx]
    } else {
        header
    };
    Some(val.trim().to_string())
}

/// Extract all lines of a specific header name, returned as a vector of full header lines (e.g. `["Via: ...", "Via: ..."]`)
pub fn extract_headers_raw(msg: &str, header_name: &str) -> Vec<String> {
    let mut res = Vec::new();
    for line in msg.lines() {
        if match_header_prefix(line, header_name).is_some() {
            res.push(line.trim().to_string());
        }
    }
    res
}

/// Reject values that would break out of the SIP request line or a header.
///
/// Request URIs and header values are interpolated into the message as-is, so a
/// caller-supplied CR/LF (or any other control character) could inject headers
/// or smuggle a second request. Callers pass values that reach us from the REST
/// API, the IPC socket, or the CLI.
pub fn validate_header_value(value: &str, what: &str) -> Result<()> {
    if value.is_empty() {
        anyhow::bail!("{} must not be empty", what);
    }
    if let Some(bad) = value.chars().find(|c| c.is_control()) {
        anyhow::bail!(
            "{} contains an illegal control character (U+{:04X})",
            what,
            bad as u32
        );
    }
    if let Some(bad) = value.chars().find(|c| matches!(c, '<' | '>' | '"')) {
        anyhow::bail!("{} contains an illegal character '{}'", what, bad);
    }
    Ok(())
}

/// Parsed Session-Expires info (RFC 4028).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionExpires {
    pub delta_seconds: u32,
    pub refresher: Option<String>,
}

/// Parse Session-Expires header (RFC 4028), e.g. "Session-Expires: 1800;refresher=uac"
pub fn parse_session_expires(msg: &str) -> Option<SessionExpires> {
    let hdr = extract_header(msg, "Session-Expires");
    if hdr.is_empty() {
        return None;
    }
    let mut parts = hdr.split(';');
    let delta_str = parts.next()?.trim();
    let delta_seconds = delta_str.parse::<u32>().ok()?;

    let mut refresher = None;
    for param in parts {
        let p = param.trim();
        if starts_with_ascii_ci(p, "refresher=") {
            refresher = Some(p["refresher=".len()..].trim().to_ascii_lowercase());
        }
    }

    Some(SessionExpires {
        delta_seconds,
        refresher,
    })
}

/// Parse Min-SE header (RFC 4028), e.g. "Min-SE: 90"
#[allow(dead_code)]
pub fn parse_min_se(msg: &str) -> Option<u32> {
    let hdr = extract_header(msg, "Min-SE");
    if hdr.is_empty() {
        return None;
    }
    hdr.split(';').next()?.trim().parse::<u32>().ok()
}

/// Parse RSeq header for PRACK (RFC 3262), e.g. "RSeq: 1001"
#[allow(dead_code)]
pub fn parse_rseq(msg: &str) -> Option<u32> {
    let hdr = extract_header(msg, "RSeq");
    if hdr.is_empty() {
        return None;
    }
    hdr.trim().parse::<u32>().ok()
}

/// Parse RAck header (RFC 3262), e.g. "RAck: 1001 1 INVITE" -> (rseq, cseq, method)
#[allow(dead_code)]
pub fn parse_rack(msg: &str) -> Option<(u32, u32, String)> {
    let hdr = extract_header(msg, "RAck");
    if hdr.is_empty() {
        return None;
    }
    let parts: Vec<&str> = hdr.split_whitespace().collect();
    if parts.len() >= 3 {
        let rseq = parts[0].parse::<u32>().ok()?;
        let cseq = parts[1].parse::<u32>().ok()?;
        let method = parts[2].to_ascii_uppercase();
        Some((rseq, cseq, method))
    } else {
        None
    }
}

/// Valid DTMF digits for out-of-band signalling (RFC 2833 / RFC 6086).
pub fn validate_dtmf_digit(digit: char) -> Result<()> {
    if digit.is_ascii_digit() || matches!(digit, '*' | '#' | 'A'..='D' | 'a'..='d') {
        Ok(())
    } else {
        anyhow::bail!("'{}' is not a valid DTMF digit (0-9, *, #, A-D)", digit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_quoted: byte-offset regressions ────────────

    /// A multi-byte character before the key used to shift the search offset
    /// (`to_lowercase()` turns the 2-byte 'İ' into 3 bytes), landing mid-character
    /// and panicking on the slice.
    #[test]
    fn extract_quoted_survives_multibyte_display_name() {
        let line = "From: \"İ\" <sip:a@b.com>;tag=şirket";
        assert_eq!(extract_quoted(line, "tag="), Some("şirket".to_string()));
    }

    /// Same offset drift, silently returning a truncated value instead of panicking.
    #[test]
    fn extract_quoted_is_not_shifted_by_multibyte_realm() {
        let line = "WWW-Authenticate: Digest realm=\"İIİI\", nonce=\"abc123\"";
        assert_eq!(extract_quoted(line, "realm="), Some("İIİI".to_string()));
        assert_eq!(extract_quoted(line, "nonce="), Some("abc123".to_string()));
    }

    #[test]
    fn extract_quoted_handles_multibyte_before_unquoted_value() {
        let line = "From: \"İİİ\" <sip:a@b>;tag=1234";
        assert_eq!(extract_quoted(line, "tag="), Some("1234".to_string()));
    }

    /// The key must sit at a parameter boundary — a `tag=` inside a display
    /// name must not shadow the real parameter.
    #[test]
    fn extract_quoted_ignores_key_inside_display_name() {
        let line = "From: \"tag=spoofed\" <sip:a@b>;tag=real";
        assert_eq!(extract_quoted(line, "tag="), Some("real".to_string()));
    }

    #[test]
    fn extract_quoted_ignores_key_suffix_of_another_param() {
        let line = "To: <sip:a@b>;xtag=spoofed;tag=real";
        assert_eq!(extract_quoted(line, "tag="), Some("real".to_string()));
    }

    #[test]
    fn extract_quoted_is_case_insensitive() {
        let line = "WWW-Authenticate: Digest REALM=\"example.com\", NONCE=\"xyz\"";
        assert_eq!(
            extract_quoted(line, "realm="),
            Some("example.com".to_string())
        );
        assert_eq!(extract_quoted(line, "nonce="), Some("xyz".to_string()));
    }

    #[test]
    fn extract_quoted_returns_none_when_absent() {
        assert_eq!(extract_quoted("To: <sip:a@b>", "tag="), None);
    }

    // ── header matching ────────────────────────────────────

    /// A line starting with U+212A (Kelvin sign) lowercases to a *shorter*
    /// byte string; matching on the lowered copy then sliced the original
    /// mid-character.
    #[test]
    fn extract_header_survives_multibyte_header_name() {
        assert_eq!(extract_header("\u{212A}: evil", "Supported"), "");
    }

    #[test]
    fn extract_header_matches_full_and_compact_names() {
        let msg = "INVITE sip:a@b SIP/2.0\r\nCall-ID: abc123\r\nf: <sip:c@d>;tag=x\r\n";
        assert_eq!(extract_header(msg, "Call-ID"), "abc123");
        assert_eq!(extract_header(msg, "From"), "<sip:c@d>;tag=x");
        assert_eq!(extract_param(msg, "From", "tag"), "x");
    }

    #[test]
    fn extract_header_is_case_insensitive() {
        assert_eq!(extract_header("CALL-ID: abc\r\n", "Call-ID"), "abc");
    }

    /// A compact alias only matches as the whole header name, not as a prefix.
    #[test]
    fn extract_header_does_not_match_partial_name() {
        assert_eq!(extract_header("Fromage: cheese\r\n", "From"), "");
        assert_eq!(extract_header("id: x\r\n", "Call-ID"), "");
    }

    #[test]
    fn extract_to_tag_reads_tag_from_to_line() {
        let resp = "SIP/2.0 200 OK\r\nTo: <sip:a@b>;tag=as12345\r\n\r\n";
        assert_eq!(extract_to_tag(resp), Some("as12345".to_string()));
    }

    /// A 407 is the proxy's challenge; answering the registrar's WWW-Authenticate
    /// instead leaves the proxy challenging forever.
    #[test]
    fn extract_auth_challenge_prefers_the_header_matching_the_status() {
        let both = "WWW-Authenticate: Digest realm=\"registrar\", nonce=\"aaa\"\r\n\
                    Proxy-Authenticate: Digest realm=\"proxy\", nonce=\"bbb\"\r\n\r\n";

        let proxied = format!("SIP/2.0 407 Proxy Authentication Required\r\n{}", both);
        let challenge = extract_auth_challenge(&proxied).expect("challenge");
        assert_eq!(challenge.realm, "proxy");
        assert!(challenge.proxy);

        let unauthorized = format!("SIP/2.0 401 Unauthorized\r\n{}", both);
        let challenge = extract_auth_challenge(&unauthorized).expect("challenge");
        assert_eq!(challenge.realm, "registrar");
        assert!(!challenge.proxy);
    }

    #[test]
    fn test_extract_all_auth_challenges() {
        let both = "SIP/2.0 401 Unauthorized\r\n\
                    WWW-Authenticate: Digest realm=\"realm-a\", nonce=\"aaa\"\r\n\
                    WWW-Authenticate: Digest realm=\"realm-b\", nonce=\"bbb\"\r\n\r\n";
        let challenges = extract_all_auth_challenges(both);
        assert_eq!(challenges.len(), 2);
        assert_eq!(challenges[0].realm, "realm-a");
        assert_eq!(challenges[1].realm, "realm-b");
    }

    /// Falls back to the other kind when the matching one is absent.
    #[test]
    fn extract_auth_challenge_falls_back_to_the_other_header() {
        let resp = "SIP/2.0 407 Proxy Authentication Required\r\n\
                    WWW-Authenticate: Digest realm=\"registrar\", nonce=\"aaa\"\r\n\r\n";
        let challenge = extract_auth_challenge(resp).expect("challenge");
        assert_eq!(challenge.realm, "registrar");
        assert!(!challenge.proxy);
    }

    /// A challenge this client cannot answer must not hide a usable one behind it.
    #[test]
    fn extract_auth_challenge_skips_challenges_without_a_nonce() {
        let resp = "SIP/2.0 401 Unauthorized\r\n\
                    WWW-Authenticate: Basic realm=\"legacy\"\r\n\
                    WWW-Authenticate: Digest realm=\"sip.example.com\", nonce=\"deadbeef\"\r\n\r\n";
        let challenge = extract_auth_challenge(resp).expect("challenge");
        assert_eq!(challenge.realm, "sip.example.com");
        assert_eq!(challenge.nonce, "deadbeef");
    }

    #[test]
    fn extract_auth_challenge_skips_unsupported_algorithm_when_md5_present() {
        let resp = "SIP/2.0 401 Unauthorized\r\n\
                    WWW-Authenticate: Digest realm=\"sip.example.com\", nonce=\"sha256nonce\", algorithm=SHA-256\r\n\
                    WWW-Authenticate: Digest realm=\"sip.example.com\", nonce=\"md5nonce\", algorithm=MD5\r\n\r\n";
        let challenge = extract_auth_challenge(resp).expect("challenge");
        assert_eq!(challenge.nonce, "md5nonce");
    }

    #[test]
    fn extract_auth_challenge_returns_none_without_a_usable_challenge() {
        assert!(extract_auth_challenge("SIP/2.0 401 Unauthorized\r\n\r\n").is_none());
        assert!(extract_auth_challenge(
            "SIP/2.0 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"x\"\r\n\r\n"
        )
        .is_none());
    }

    #[test]
    fn extract_auth_challenge_parses_qop_opaque_stale() {
        let resp = "SIP/2.0 401 Unauthorized\r\n\
                    WWW-Authenticate: Digest realm=\"sip.example.com\", nonce=\"deadbeef\", \
                    opaque=\"xyz\", qop=\"auth,auth-int\", algorithm=MD5, stale=false\r\n\r\n";
        let c = extract_auth_challenge(resp).unwrap();
        assert_eq!(c.realm, "sip.example.com");
        assert_eq!(c.nonce, "deadbeef");
        assert_eq!(c.opaque.as_deref(), Some("xyz"));
        assert!(c.supports_qop_auth());
        assert!(!c.stale);
        assert_eq!(c.algorithm.as_deref(), Some("MD5"));
    }

    /// An INVITE crafted to panic the incoming-call watcher must parse cleanly.
    #[test]
    fn hostile_invite_parses_without_panicking() {
        let msg = "INVITE sip:me@here SIP/2.0\r\n\
                   Via: SIP/2.0/UDP 10.0.0.1;branch=z9hG4bK-İ\r\n\
                   From: \"İ\" <sip:attacker@evil.test>;tag=şirket\r\n\
                   To: <sip:me@here>\r\n\
                   Call-ID: İIİI@evil.test\r\n\
                   CSeq: 1 INVITE\r\n\r\n";
        assert_eq!(extract_param(msg, "From", "tag"), "şirket");
        assert_eq!(extract_header(msg, "Call-ID"), "İIİI@evil.test");
        assert_eq!(
            extract_uri(&extract_header(msg, "From")),
            Some("sip:attacker@evil.test".to_string())
        );
        assert_eq!(extract_headers_raw(msg, "Via").len(), 1);
    }

    // ── outgoing-value validation ──────────────────────────

    #[test]
    fn validate_header_value_rejects_crlf_injection() {
        assert!(validate_header_value("sip:bob@example.com", "target").is_ok());
        assert!(
            validate_header_value("sip:b@e.com\r\nSubject: injected", "target").is_err(),
            "CRLF must be rejected"
        );
        assert!(validate_header_value("sip:b@e.com\nX: y", "target").is_err());
        assert!(validate_header_value("sip:b@e.com\r", "target").is_err());
        assert!(validate_header_value("sip:b@e.com\0", "target").is_err());
        assert!(validate_header_value("", "target").is_err());
    }

    #[test]
    fn validate_header_value_allows_unicode() {
        assert!(validate_header_value("sip:şirket@example.com", "target").is_ok());
    }

    #[test]
    fn validate_dtmf_digit_accepts_only_dtmf_alphabet() {
        for c in "0123456789*#ABCDabcd".chars() {
            assert!(validate_dtmf_digit(c).is_ok(), "{} should be valid", c);
        }
        for c in ['\r', '\n', 'z', ' ', '\0'] {
            assert!(validate_dtmf_digit(c).is_err(), "{:?} should be invalid", c);
        }
    }

    #[test]
    fn validate_header_value_rejects_angle_brackets_and_quotes() {
        assert!(validate_header_value("sip:bob@example.com", "target").is_ok());
        assert!(validate_header_value("sip:b@e.com;transport=tcp", "target").is_ok());
        assert!(validate_header_value("<sip:b@e.com>", "target").is_err());
        assert!(validate_header_value("sip:b@e.com>", "target").is_err());
        assert!(validate_header_value("sip:\"b\"@e.com", "target").is_err());
    }

    #[test]
    fn test_parse_session_expires() {
        let msg = "SIP/2.0 200 OK\r\nSession-Expires: 1800;refresher=uac\r\nMin-SE: 90\r\n\r\n";
        let se = parse_session_expires(msg).expect("valid session-expires");
        assert_eq!(se.delta_seconds, 1800);
        assert_eq!(se.refresher.as_deref(), Some("uac"));

        let min_se = parse_min_se(msg).expect("valid min-se");
        assert_eq!(min_se, 90);

        let msg_plain = "SIP/2.0 200 OK\r\nSession-Expires: 3600\r\n\r\n";
        let se2 = parse_session_expires(msg_plain).expect("plain se");
        assert_eq!(se2.delta_seconds, 3600);
        assert_eq!(se2.refresher, None);
    }

    #[test]
    fn test_parse_rseq_and_rack() {
        let msg_183 = "SIP/2.0 183 Session Progress\r\nRSeq: 1001\r\nRequire: 100rel\r\n\r\n";
        assert_eq!(parse_rseq(msg_183), Some(1001));

        let msg_prack = "PRACK sip:bob@example.com SIP/2.0\r\nRAck: 1001 1 INVITE\r\n\r\n";
        let rack = parse_rack(msg_prack).expect("valid rack");
        assert_eq!(rack, (1001, 1, "INVITE".to_string()));
    }
}
