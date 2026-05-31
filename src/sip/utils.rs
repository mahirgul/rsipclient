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

/// Extract the `tag` parameter from the To header.
pub fn extract_to_tag(response: &str) -> Option<String> {
    let to_line = response
        .lines()
        .find(|l| l.to_lowercase().starts_with("to:"))?;
    extract_quoted(to_line, "tag=")
}

/// Extract a quoted (or unquoted) parameter value from a SIP header line.
///
/// Supports:
///   `realm="sip.example.com"`  (quoted)
///   `expires=3600`             (unquoted)
pub fn extract_quoted(line: &str, key: &str) -> Option<String> {
    let start = line.find(key)? + key.len();
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
    let prefix = format!("{}:", header_name);
    msg.lines()
        .find(|l| l.to_lowercase().starts_with(&prefix.to_lowercase()))
        .map(|l| l[prefix.len()..].trim().to_string())
        .unwrap_or_default()
}

/// Extract a named parameter from a SIP header line.
/// e.g. `extract_param(msg, "From", "tag")` → "abc123"
pub fn extract_param(msg: &str, header_name: &str, param: &str) -> String {
    let prefix = format!("{}:", header_name);
    let line = msg
        .lines()
        .find(|l| l.to_lowercase().starts_with(&prefix.to_lowercase()));
    match line {
        Some(l) => extract_quoted(l, &format!("{}=", param)).unwrap_or_default(),
        None => String::new(),
    }
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
    let prefix = format!("{}:", header_name).to_lowercase();
    msg.lines()
        .filter(|l| l.to_lowercase().starts_with(&prefix))
        .map(|l| l.trim().to_string())
        .collect()
}
