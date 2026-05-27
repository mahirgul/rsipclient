//! SIP account settings - optional per-account configuration

/// Bundles all optional SIP account settings
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct SipSettings {
    /// Display name for From/To headers (e.g. "Alice Smith")
    pub display_name: Option<String>,

    /// P-Asserted-Identity URI (RFC 3325) - trusted network caller ID
    pub asserted_id: Option<String>,

    /// P-Preferred-Identity URI (RFC 3325) - preferred caller ID
    pub preferred_id: Option<String>,

    /// Outbound proxy (host:port), bypasses server for routing
    pub proxy: Option<String>,

    /// Registration expiry in seconds (default: 3600)
    pub register_expiry: u32,

    /// Custom User-Agent header value
    pub user_agent: Option<String>,

    /// DTMF mode: "rfc2833", "inband", or "info"
    pub dtmf_mode: Option<String>,

    /// Accept early media (183 Session Progress)
    pub early_media: bool,

    /// Support RFC 4028 session timers
    pub session_timers: bool,
}

impl Default for SipSettings {
    fn default() -> Self {
        SipSettings {
            display_name: None,
            asserted_id: None,
            preferred_id: None,
            proxy: None,
            register_expiry: 3600,
            user_agent: None,
            dtmf_mode: None,
            early_media: true,
            session_timers: false,
        }
    }
}

impl SipSettings {
    /// Build from optional config values
    pub fn from_config(
        display_name: Option<String>,
        asserted_id: Option<String>,
        preferred_id: Option<String>,
        proxy: Option<String>,
        register_expiry: Option<u32>,
        user_agent: Option<String>,
        dtmf_mode: Option<String>,
        early_media: Option<bool>,
        session_timers: Option<bool>,
    ) -> Self {
        SipSettings {
            display_name,
            asserted_id,
            preferred_id,
            proxy,
            register_expiry: register_expiry.unwrap_or(3600),
            user_agent,
            dtmf_mode,
            early_media: early_media.unwrap_or(true),
            session_timers: session_timers.unwrap_or(false),
        }
    }

    /// Build the From header display part:
    ///   `"Alice Smith" <sip:user@domain>`
    ///   or `<sip:user@domain>`
    pub fn format_from(&self, username: &str, domain: &str) -> String {
        match &self.display_name {
            Some(ref name) => format!("\"{}\" <sip:{}@{}>", name, username, domain),
            None => format!("<sip:{}@{}>", username, domain),
        }
    }

    /// Build extra SIP header lines based on settings
    pub fn extra_headers(&self) -> String {
        let mut h = String::new();

        if let Some(ref id) = self.asserted_id {
            h.push_str(&format!("P-Asserted-Identity: <{}>\r\n", id));
        }
        if let Some(ref id) = self.preferred_id {
            h.push_str(&format!("P-Preferred-Identity: <{}>\r\n", id));
        }
        if let Some(ref ua) = self.user_agent {
            h.push_str(&format!("User-Agent: {}\r\n", ua));
        }
        if self.session_timers {
            h.push_str("Session-Expires: 1800;refresher=uac\r\n");
        }
        if self.early_media {
            h.push_str("Supported: 100rel, timer\r\n");
        }

        h
    }
}
