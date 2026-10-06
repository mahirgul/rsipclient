//! SIP message builders module
//!
//! Submodules organize raw SIP request string construction:
//! - [`register`]: REGISTER requests
//! - [`invite`]: INVITE, ACK, BYE, CANCEL requests
//! - [`rfc_extensions`]: PRACK, MESSAGE, INFO DTMF, SUBSCRIBE requests

pub mod invite;
pub mod register;
pub mod rfc_extensions;

pub use invite::{build_ack, build_bye, build_cancel, build_invite, build_invite_with_auth};
pub use register::{build_register, build_register_with_auth};
pub use rfc_extensions::{build_info_dtmf, build_message, build_prack, build_subscribe};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sip::settings::SipSettings;

    #[test]
    fn test_rfc3326_reason_header() {
        let settings = SipSettings::default();
        let cancel = build_cancel(
            "alice",
            "example.com",
            "sip:bob@example.com",
            "192.168.1.10:5060",
            "tag-1",
            "callid-1",
            1,
            "branch-1",
            "",
            &settings,
            "udp",
        );
        assert!(cancel.contains("Reason: Q.850;cause=16;text=\"Normal call clearing\""));
    }

    #[test]
    fn test_rfc3262_prack_builder() {
        let settings = SipSettings::default();
        let prack = build_prack(
            "sip:bob@example.com",
            "alice",
            "example.com",
            "192.168.1.10:5060",
            "tag-1",
            "tag-remote",
            "callid-1",
            2,
            1001,
            1,
            "branch-1",
            &settings,
            "udp",
        );
        assert!(prack.contains("PRACK sip:bob@example.com SIP/2.0"));
        assert!(prack.contains("RAck: 1001 1 INVITE"));
    }

    #[test]
    fn test_rfc3428_message_builder() {
        let settings = SipSettings::default();
        let msg = build_message(
            "sip:bob@example.com",
            "alice",
            "example.com",
            "192.168.1.10:5060",
            "tag-1",
            "branch-1",
            "callid-1",
            1,
            "Hello World!",
            &settings,
            "udp",
        );
        assert!(msg.contains("MESSAGE sip:bob@example.com SIP/2.0"));
        assert!(msg.contains("Content-Type: text/plain;charset=UTF-8"));
        assert!(msg.contains("Hello World!"));
    }

    #[test]
    fn test_rfc6086_info_dtmf_builder() {
        let settings = SipSettings::default();
        let info = build_info_dtmf(
            "sip:bob@example.com",
            "alice",
            "example.com",
            "192.168.1.10:5060",
            "tag-1",
            "tag-remote",
            "callid-1",
            2,
            "branch-1",
            "",
            '5',
            250,
            &settings,
            "udp",
        );
        assert!(info.contains("INFO sip:bob@example.com SIP/2.0"));
        assert!(info.contains("Signal=5"));
        assert!(info.contains("Duration=250"));
    }

    use crate::sip::utils::{extract_header, AuthChallenge};

    /// Check the framing every SIP request must have: CRLF line endings, a
    /// blank line after the headers, and a Content-Length equal to the body
    /// size in bytes. Returns (start line, body).
    fn assert_well_formed(msg: &str) -> (&str, &str) {
        let (head, body) = msg.split_once("\r\n\r\n").expect("header/body separator");
        assert!(
            !head.contains('\n') || head.split("\r\n").all(|l| !l.contains('\n')),
            "bare LF in headers"
        );
        for line in head.split("\r\n").skip(1) {
            assert!(line.contains(':'), "malformed header line {line:?}");
        }
        let cl: usize = extract_header(msg, "Content-Length")
            .trim()
            .parse()
            .unwrap();
        assert_eq!(cl, body.len(), "Content-Length mismatch");
        (head.split("\r\n").next().unwrap(), body)
    }

    fn challenge(proxy: bool) -> AuthChallenge {
        AuthChallenge {
            realm: "example.com".into(),
            nonce: "abc123".into(),
            qop: Some("auth".into()),
            proxy,
            ..Default::default()
        }
    }

    #[test]
    fn register_is_well_formed_with_stable_instance_id() {
        let settings = SipSettings::default();
        let (msg, call_id, cseq) = build_register(
            "1001",
            "example.com",
            "10.0.0.5:5060",
            "tagA",
            "z9hG4bK1",
            "cid@x",
            7,
            &settings,
            "udp",
        );
        assert_eq!(call_id, "cid@x");
        assert_eq!(cseq, 7);
        let (start, body) = assert_well_formed(&msg);
        assert_eq!(start, "REGISTER sip:example.com SIP/2.0");
        assert!(body.is_empty());
        assert!(msg.contains("Via: SIP/2.0/UDP 10.0.0.5:5060;branch=z9hG4bK1;rport\r\n"));
        assert!(msg.contains("To: <sip:1001@example.com>\r\n"));
        assert!(msg.contains("CSeq: 7 REGISTER\r\n"));
        assert!(msg.contains("Expires: 3600\r\n"));
        assert!(!msg.contains("Authorization"));

        let again = build_register(
            "1001",
            "example.com",
            "10.0.0.9:6000",
            "tagB",
            "z9hG4bK2",
            "cid2",
            8,
            &settings,
            "udp",
        )
        .0;
        let instance = |m: &str| {
            extract_header(m, "Contact")
                .split("urn:uuid:")
                .nth(1)
                .unwrap()
                .to_string()
        };
        assert_eq!(instance(&msg), instance(&again));
        let uuid = instance(&msg);
        let uuid = uuid.trim_end_matches(">\"");
        assert_eq!(uuid.len(), 36);
        assert_eq!(uuid.matches('-').count(), 4);
    }

    #[test]
    fn register_with_auth_adds_the_matching_header() {
        let settings = SipSettings {
            register_expiry: 120,
            ..Default::default()
        };
        for (proxy, header) in [(false, "Authorization"), (true, "Proxy-Authorization")] {
            let msg = build_register_with_auth(
                "1001",
                "pw",
                "example.com",
                "10.0.0.5:5060",
                "t",
                "b",
                "c",
                2,
                &challenge(proxy),
                &settings,
                "tls",
            );
            assert_well_formed(&msg);
            let auth = extract_header(&msg, header);
            assert!(!auth.is_empty(), "missing {header}");
            assert!(auth.contains("uri=\"sip:example.com\""));
            assert!(auth.contains("qop=auth"));
            assert!(msg.contains("Via: SIP/2.0/TLS "));
            assert!(msg.contains("Contact: <sips:1001@10.0.0.5:5060>"));
            assert!(msg.contains("Expires: 120\r\n"));
        }
    }

    #[test]
    fn invite_content_length_counts_bytes() {
        let settings = SipSettings {
            display_name: Some("Ali \"Veli\" Çelik".into()),
            asserted_id: Some("sip:1001@example.com".into()),
            ..Default::default()
        };
        let sdp = "v=0\r\ns=Görüşme\r\n";
        let msg = build_invite(
            "<sip:bob@example.com>",
            "alice",
            "example.com",
            "10.0.0.5:5060",
            "t",
            "b",
            "c",
            1,
            sdp,
            &settings,
            "tcp",
        );
        let (start, body) = assert_well_formed(&msg);
        assert_eq!(start, "INVITE sip:bob@example.com SIP/2.0");
        assert_eq!(body, sdp);
        assert!(msg.contains("From: \"Ali Veli Çelik\" <sip:alice@example.com>;tag=t\r\n"));
        assert!(msg.contains("P-Asserted-Identity: <sip:1001@example.com>\r\n"));
        assert!(msg.contains("Content-Type: application/sdp\r\n"));
        assert!(msg.contains("Via: SIP/2.0/TCP "));
    }

    #[test]
    fn invite_with_auth_signs_the_request_uri() {
        let settings = SipSettings::default();
        let msg = build_invite_with_auth(
            "sip:bob@example.com",
            "alice",
            "pw",
            "example.com",
            "10.0.0.5:5060",
            "t",
            "b",
            "c",
            2,
            "v=0\r\n",
            &challenge(true),
            &settings,
            "udp",
        );
        assert_well_formed(&msg);
        let auth = extract_header(&msg, "Proxy-Authorization");
        assert!(auth.contains("uri=\"sip:bob@example.com\""));
        assert!(msg.contains("CSeq: 2 INVITE\r\n"));
    }

    #[test]
    fn in_dialog_requests_carry_tags_and_routes() {
        let settings = SipSettings::default();
        let route = "Route: <sip:proxy.example.com;lr>\r\n";
        let ack = build_ack(
            "sip:bob@10.0.0.9",
            "alice",
            "example.com",
            "10.0.0.5:5060",
            "lt",
            "rt",
            "c",
            3,
            "b1",
            route,
            &settings,
            "udp",
        );
        let bye = build_bye(
            "alice",
            "example.com",
            "sip:bob@10.0.0.9",
            "10.0.0.5:5060",
            "lt",
            "rt",
            "c",
            4,
            "b2",
            route,
            &settings,
            "udp",
        );
        let cancel = build_cancel(
            "alice",
            "example.com",
            "sip:bob@10.0.0.9",
            "10.0.0.5:5060",
            "lt",
            "c",
            3,
            "b3",
            "",
            &settings,
            "udp",
        );
        for (msg, method, cseq) in [(&ack, "ACK", 3), (&bye, "BYE", 4), (&cancel, "CANCEL", 3)] {
            let (start, body) = assert_well_formed(msg);
            assert_eq!(start, format!("{method} sip:bob@10.0.0.9 SIP/2.0"));
            assert!(body.is_empty());
            assert!(msg.contains(&format!("CSeq: {cseq} {method}\r\n")));
            assert!(msg.contains("From: <sip:alice@example.com>;tag=lt\r\n"));
        }
        assert!(ack.contains(route) && bye.contains(route));
        assert!(ack.contains("To: <sip:bob@10.0.0.9>;tag=rt\r\n"));
        assert!(bye.contains("To: <sip:bob@10.0.0.9>;tag=rt\r\n"));
        // CANCEL matches the INVITE, which had no To tag (RFC 3261 §9.1).
        assert!(cancel.contains("To: <sip:bob@10.0.0.9>\r\n"));
    }
}
