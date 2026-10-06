//! SIP header parsing on arbitrary (possibly non-UTF-8) input.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rsipclient::sip::utils;

fuzz_target!(|data: &[u8]| {
    let msg = String::from_utf8_lossy(data);
    let _ = utils::parse_status_code(&msg);
    let _ = utils::extract_all_auth_challenges(&msg);
    let _ = utils::extract_to_tag(&msg);
    for name in ["Via", "To", "From", "Contact", "Call-ID", "CSeq"] {
        let value = utils::extract_header(&msg, name);
        let _ = utils::extract_uri(&value);
        let _ = utils::split_header_values(&value);
        let _ = utils::extract_param(&msg, name, "tag");
    }
    let _ = utils::format_route_headers(&utils::extract_record_routes(&msg));
    let _ = utils::parse_session_expires(&msg);
    let _ = utils::parse_rack(&msg);
    let _ = rsipclient::sip::transaction::build_prack_200_ok(&msg);
});
