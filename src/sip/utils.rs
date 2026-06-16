//! SIP utility functions: parsing status codes, extracting headers, ID generation

use anyhow::{Context, Result};
use uuid::Uuid;

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

/// Extract realm and nonce from WWW-Authenticate / Proxy-Authenticate header.
pub fn extract_auth_params(response: &str) -> Option<(String, String)> {
    let header = response.lines().find(|l| {
        let lower = l.to_lowercase();
        lower.starts_with("www-authenticate:") || lower.starts_with("proxy-authenticate:")
    })?;

    let realm = extract_quoted(header, "realm=")?;
    let nonce = extract_quoted(header, "nonce=")?;

    Some((realm, nonce))
}

/// Match a SIP header line against a full header name or its compact alias.
/// Returns the length of the matching prefix if it matches, else None.
fn match_header_prefix(line: &str, name: &str) -> Option<usize> {
    let lower_line = line.to_lowercase();
    let lower_name = name.to_lowercase();

    // Check full name
    let full_prefix = format!("{}:", lower_name);
    if lower_line.starts_with(&full_prefix) {
        return Some(full_prefix.len());
    }

    // Check compact name alias (RFC 3261 Section 7.3.3)
    let compact = match lower_name.as_str() {
        "call-id" => Some("i"),
        "from" => Some("f"),
        "to" => Some("t"),
        "via" => Some("v"),
        "contact" => Some("m"),
        "content-type" => Some("c"),
        "content-length" => Some("l"),
        "subject" => Some("s"),
        "supported" => Some("k"),
        "content-encoding" => Some("e"),
        "accept-encoding" => Some("a"),
        _ => None,
    };

    if let Some(c) = compact {
        let compact_prefix = format!("{}:", c);
        if lower_line.starts_with(&compact_prefix) {
            return Some(compact_prefix.len());
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

/// Extract a quoted (or unquoted) parameter value from a SIP header line.
///
/// Supports:
///   `realm="sip.example.com"`  (quoted)
///   `expires=3600`             (unquoted)
pub fn extract_quoted(line: &str, key: &str) -> Option<String> {
    let lower_line = line.to_lowercase();
    let lower_key = key.to_lowercase();
    let start = lower_line.find(&lower_key)? + key.len();
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
