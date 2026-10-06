//! Shared helpers for integration tests: a scripted SIP peer on UDP and a
//! client factory.
#![allow(dead_code)]

use rsipclient::sip::client::{AuthMethod, SipClient};
use rsipclient::sip::settings::SipSettings;
use rsipclient::sip::transport::Transport;
use rsipclient::sip::utils::extract_header;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::UdpSocket;

pub const USER: &str = "1001";
pub const PASSWORD: &str = "s3cret";
pub const DOMAIN: &str = "example.com";
pub const REALM: &str = "example.com";

/// A SIP server/registrar/peer driven step by step by the test.
pub struct MockSipServer {
    sock: UdpSocket,
    pub addr: SocketAddr,
}

impl MockSipServer {
    pub async fn start() -> Self {
        let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = sock.local_addr().unwrap();
        Self { sock, addr }
    }

    /// Next SIP request from the client, skipping keep-alives and, unless
    /// `method` is "ACK", the ACKs the transaction layer sends for errors.
    pub async fn expect(&self, method: &str) -> (String, SocketAddr) {
        let mut buf = vec![0u8; 65536];
        loop {
            let (n, from) =
                tokio::time::timeout(Duration::from_secs(5), self.sock.recv_from(&mut buf))
                    .await
                    .unwrap_or_else(|_| panic!("timed out waiting for {method}"))
                    .unwrap();
            let msg = String::from_utf8_lossy(&buf[..n]).into_owned();
            if msg.trim().is_empty() {
                continue;
            }
            let got = msg.split_whitespace().next().unwrap_or("").to_string();
            if got == "ACK" && method != "ACK" {
                continue;
            }
            // Retransmissions of an earlier request may still be in flight.
            if got != method {
                continue;
            }
            return (msg, from);
        }
    }

    pub async fn send(&self, msg: &str, to: SocketAddr) {
        self.sock.send_to(msg.as_bytes(), to).await.unwrap();
    }

    /// Answer `req` with a response copying its transaction headers.
    pub async fn respond(
        &self,
        req: &str,
        to: SocketAddr,
        status: &str,
        to_tag: Option<&str>,
        extra: &str,
        body: &str,
    ) {
        let to_hdr = extract_header(req, "To");
        let to_hdr = match to_tag {
            Some(tag) if !to_hdr.contains("tag=") => format!("{to_hdr};tag={tag}"),
            _ => to_hdr,
        };
        let msg = format!(
            "SIP/2.0 {status}\r\n\
             Via: {via}\r\n\
             From: {from}\r\n\
             To: {to_hdr}\r\n\
             Call-ID: {cid}\r\n\
             CSeq: {cseq}\r\n\
             {extra}\
             Content-Length: {len}\r\n\
             \r\n\
             {body}",
            via = extract_header(req, "Via"),
            from = extract_header(req, "From"),
            cid = extract_header(req, "Call-ID"),
            cseq = extract_header(req, "CSeq"),
            len = body.len(),
        );
        self.send(&msg, to).await;
    }
}

/// Parse `key=value` / `key="value"` parameters of a Digest header.
pub fn digest_params(header: &str) -> std::collections::HashMap<String, String> {
    let rest = header
        .trim()
        .strip_prefix("Digest")
        .unwrap_or(header)
        .trim();
    let mut out = std::collections::HashMap::new();
    for part in rest.split(',') {
        if let Some((k, v)) = part.trim().split_once('=') {
            out.insert(
                k.trim().to_ascii_lowercase(),
                v.trim().trim_matches('"').to_string(),
            );
        }
    }
    out
}

/// Recompute the RFC 2617 digest the way a registrar would and compare.
pub fn digest_is_valid(header: &str, method: &str, password: &str) -> bool {
    let p = digest_params(header);
    let get = |k: &str| p.get(k).cloned().unwrap_or_default();
    let ha1 = format!(
        "{:x}",
        md5::compute(format!("{}:{}:{}", get("username"), get("realm"), password))
    );
    let ha2 = format!("{:x}", md5::compute(format!("{}:{}", method, get("uri"))));
    let expected = if p.contains_key("qop") {
        format!(
            "{:x}",
            md5::compute(format!(
                "{}:{}:{}:{}:{}:{}",
                ha1,
                get("nonce"),
                get("nc"),
                get("cnonce"),
                get("qop"),
                ha2
            ))
        )
    } else {
        format!(
            "{:x}",
            md5::compute(format!("{}:{}:{}", ha1, get("nonce"), ha2))
        )
    };
    expected == get("response")
}

/// A UDP client for `server` with digest auth enabled.
pub async fn client_for(server: SocketAddr, password: &str) -> SipClient {
    let transport = Transport::new_udp("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let local = transport.local_addr().unwrap();
    SipClient::new(
        transport,
        server,
        local,
        USER.into(),
        password.into(),
        DOMAIN.into(),
        42000,
        42100,
        AuthMethod::Md5,
        SipSettings::default(),
        "pcmu".into(),
    )
    .await
    .unwrap()
}
