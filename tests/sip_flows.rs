//! End-to-end SIP flows against a scripted UDP peer.

mod common;

use common::*;
use rsipclient::sip::utils::extract_header;

fn challenge_header(nonce: &str, stale: bool) -> String {
    format!(
        "WWW-Authenticate: Digest realm=\"{REALM}\", nonce=\"{nonce}\", qop=\"auth\", algorithm=MD5{}\r\n",
        if stale { ", stale=true" } else { "" }
    )
}

#[tokio::test]
async fn register_answers_a_digest_challenge() {
    let server = MockSipServer::start().await;
    let client = client_for(server.addr, PASSWORD).await;

    let registrar = async {
        let (req, from) = server.expect("REGISTER").await;
        assert!(extract_header(&req, "Authorization").is_empty());
        let call_id = extract_header(&req, "Call-ID");
        server
            .respond(
                &req,
                from,
                "401 Unauthorized",
                Some("srv"),
                &challenge_header("n1", false),
                "",
            )
            .await;

        let (req2, from) = server.expect("REGISTER").await;
        // RFC 3261 §22.4: the retry keeps the Call-ID and bumps the CSeq.
        assert_eq!(extract_header(&req2, "Call-ID"), call_id);
        let cseq = |m: &str| {
            extract_header(m, "CSeq")
                .split_whitespace()
                .next()
                .unwrap()
                .parse::<u32>()
                .unwrap()
        };
        assert_eq!(cseq(&req2), cseq(&req) + 1);
        let auth = extract_header(&req2, "Authorization");
        assert!(
            digest_is_valid(&auth, "REGISTER", PASSWORD),
            "bad digest: {auth}"
        );
        assert_eq!(digest_params(&auth)["uri"], format!("sip:{DOMAIN}"));
        server
            .respond(&req2, from, "200 OK", Some("srv"), "Expires: 3600\r\n", "")
            .await;
    };

    let (ok, ()) = tokio::join!(client.register(), registrar);
    assert!(ok.unwrap());
    assert!(*client.registered.lock().await);
}

#[tokio::test]
async fn register_reports_failure_for_a_wrong_password() {
    let server = MockSipServer::start().await;
    let client = client_for(server.addr, "wrong").await;

    let registrar = async {
        let (req, from) = server.expect("REGISTER").await;
        server
            .respond(
                &req,
                from,
                "401 Unauthorized",
                Some("srv"),
                &challenge_header("n1", false),
                "",
            )
            .await;
        let (req2, from) = server.expect("REGISTER").await;
        assert!(!digest_is_valid(
            &extract_header(&req2, "Authorization"),
            "REGISTER",
            PASSWORD
        ));
        server
            .respond(&req2, from, "403 Forbidden", Some("srv"), "", "")
            .await;
    };

    let (ok, ()) = tokio::join!(client.register(), registrar);
    assert!(!ok.unwrap());
    assert!(!*client.registered.lock().await);
}

#[tokio::test]
async fn register_retries_once_on_a_stale_nonce() {
    let server = MockSipServer::start().await;
    let client = client_for(server.addr, PASSWORD).await;

    let registrar = async {
        let (req, from) = server.expect("REGISTER").await;
        server
            .respond(
                &req,
                from,
                "401 Unauthorized",
                Some("srv"),
                &challenge_header("old", false),
                "",
            )
            .await;
        let (req2, from) = server.expect("REGISTER").await;
        server
            .respond(
                &req2,
                from,
                "401 Unauthorized",
                Some("srv"),
                &challenge_header("fresh", true),
                "",
            )
            .await;
        let (req3, from) = server.expect("REGISTER").await;
        let auth = extract_header(&req3, "Authorization");
        assert_eq!(digest_params(&auth)["nonce"], "fresh");
        assert!(digest_is_valid(&auth, "REGISTER", PASSWORD));
        server
            .respond(&req3, from, "200 OK", Some("srv"), "", "")
            .await;
    };

    let (ok, ()) = tokio::join!(client.register(), registrar);
    assert!(ok.unwrap());
}

#[tokio::test]
async fn register_without_challenge_succeeds_directly() {
    let server = MockSipServer::start().await;
    let client = client_for(server.addr, PASSWORD).await;
    let registrar = async {
        let (req, from) = server.expect("REGISTER").await;
        assert!(req.contains("Expires: 3600\r\n"));
        server
            .respond(&req, from, "200 OK", Some("srv"), "", "")
            .await;
    };
    let (ok, ()) = tokio::join!(client.register(), registrar);
    assert!(ok.unwrap());
}

#[tokio::test]
async fn outgoing_call_setup_ack_and_bye() {
    let server = MockSipServer::start().await;
    let mut client = client_for(server.addr, PASSWORD).await;
    let peer_rtp = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let rtp_port = peer_rtp.local_addr().unwrap().port();

    let callee = async {
        let (inv, from) = server.expect("INVITE").await;
        assert!(inv.starts_with(&format!("INVITE sip:bob@{DOMAIN} SIP/2.0\r\n")));
        let offer = inv.split("\r\n\r\n").nth(1).unwrap();
        assert!(offer.contains("m=audio "), "INVITE must carry an SDP offer");
        assert!(offer.contains("a=rtpmap:0 PCMU/8000"));

        server.respond(&inv, from, "100 Trying", None, "", "").await;
        server
            .respond(&inv, from, "180 Ringing", Some("bobtag"), "", "")
            .await;
        let answer = format!(
            "v=0\r\no=bob 1 1 IN IP4 127.0.0.1\r\ns=-\r\nc=IN IP4 127.0.0.1\r\nt=0 0\r\n\
             m=audio {rtp_port} RTP/AVP 0 101\r\na=rtpmap:0 PCMU/8000\r\na=sendrecv\r\n"
        );
        let contact = format!(
            "Contact: <sip:bob@{}>\r\nContent-Type: application/sdp\r\n",
            server.addr
        );
        server
            .respond(&inv, from, "200 OK", Some("bobtag"), &contact, &answer)
            .await;

        let (ack, _) = server.expect("ACK").await;
        assert!(extract_header(&ack, "To").contains("tag=bobtag"));
        assert_eq!(
            extract_header(&ack, "Call-ID"),
            extract_header(&inv, "Call-ID")
        );
        let inv_cseq = extract_header(&inv, "CSeq")
            .split_whitespace()
            .next()
            .unwrap()
            .to_string();
        assert_eq!(extract_header(&ack, "CSeq"), format!("{inv_cseq} ACK"));
        // ACK for a 2xx goes to the Contact of the answer (RFC 3261 §13.2.2.4).
        assert!(ack.starts_with(&format!("ACK sip:bob@{} SIP/2.0", server.addr)));
        inv
    };

    let (ok, inv) = tokio::join!(client.invite("bob"), callee);
    assert!(ok.unwrap(), "call should be established");
    assert!(client.in_call);
    assert_eq!(client.remote_tag.as_deref(), Some("bobtag"));
    assert_eq!(client.remote_rtp_addr, Some(peer_rtp.local_addr().unwrap()));

    let hangup = async {
        let (bye, from) = server.expect("BYE").await;
        assert_eq!(
            extract_header(&bye, "Call-ID"),
            extract_header(&inv, "Call-ID")
        );
        assert!(extract_header(&bye, "To").contains("tag=bobtag"));
        server.respond(&bye, from, "200 OK", None, "", "").await;
    };
    let (ok, ()) = tokio::join!(client.bye(), hangup);
    assert!(ok.unwrap());
    assert!(!client.in_call);
    assert!(client.remote_tag.is_none());
}

#[tokio::test]
async fn rejected_call_clears_dialog_state() {
    let server = MockSipServer::start().await;
    let mut client = client_for(server.addr, PASSWORD).await;
    let callee = async {
        let (inv, from) = server.expect("INVITE").await;
        server
            .respond(&inv, from, "486 Busy Here", Some("busy"), "", "")
            .await;
        // The INVITE client transaction ACKs a non-2xx final response itself.
        let (ack, _) = server.expect("ACK").await;
        assert_eq!(extract_header(&ack, "Via"), extract_header(&inv, "Via"));
    };
    let (ok, ()) = tokio::join!(client.invite("sip:bob@example.com"), callee);
    assert!(!ok.unwrap());
    assert!(!client.in_call);
    assert!(client.remote_uri.is_none());
}
