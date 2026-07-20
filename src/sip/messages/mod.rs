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
            '5',
            250,
            &settings,
            "udp",
        );
        assert!(info.contains("INFO sip:bob@example.com SIP/2.0"));
        assert!(info.contains("Signal=5"));
        assert!(info.contains("Duration=250"));
    }
}
