//! SIP registration operations (REGISTER and UNREGISTER).
//!
//! This file implements the REGISTER and UNREGISTER SIP methods on the `SipClient` struct,
//! supporting MD5 digest authentication challenges (WWW-Authenticate / Proxy-Authenticate).

use crate::sip::client::SipClient;
use crate::sip::messages::{build_register, build_register_with_auth};
use crate::sip::utils;
use anyhow::{Context, Result};

impl SipClient {
    /// REGISTER with the server. Returns true on success.
    /// Sends an initial REGISTER request, handles 401/407 authentication
    /// challenge by retrying with calculated MD5 digest credentials.
    pub async fn register(&self) -> Result<bool> {
        let local = self.local_addr_str();
        let branch = self.new_branch();
        let call_id = self.new_call_id();
        let cseq = self.next_cseq().await;

        let (msg, _cid, _cs) = build_register(
            &self.username,
            &self.domain,
            &local,
            &self.local_tag,
            &branch,
            &call_id,
            cseq,
            &self.settings,
            self.transport.via_str(),
        );

        let resp = self.send(&msg).await?;
        let status = utils::parse_status_code(&resp)?;

        // Handle MD5 Auth challenge (401 Unauthorized or 407 Proxy Authentication Required)
        if (status == 401 || status == 407) && self.auth_method == crate::sip::AuthMethod::Md5 {
            let challenge = utils::extract_auth_challenge(&resp)
                .context("Cannot extract WWW-Authenticate params")?;

            let auth_msg = build_register_with_auth(
                &self.username,
                &self.password,
                &self.domain,
                &local,
                &self.local_tag,
                &self.new_branch(),
                // Reuse original Call-ID for auth retry (RFC 3261 §22.4)
                &call_id,
                self.next_cseq().await,
                &challenge,
                &self.settings,
                self.transport.via_str(),
            );

            let mut resp2 = self.send(&auth_msg).await?;
            let mut status2 = utils::parse_status_code(&resp2)?;

            // A server whose nonce expired answers stale=true with a fresh one;
            // the same credentials succeed when retried against it.
            if let Some(fresh) = stale_retry_challenge(status2, &resp2) {
                log::info!("Registration nonce was stale, retrying with the fresh one");
                let retry_msg = build_register_with_auth(
                    &self.username,
                    &self.password,
                    &self.domain,
                    &local,
                    &self.local_tag,
                    &self.new_branch(),
                    &call_id,
                    self.next_cseq().await,
                    &fresh,
                    &self.settings,
                    self.transport.via_str(),
                );
                resp2 = self.send(&retry_msg).await?;
                status2 = utils::parse_status_code(&resp2)?;
            }

            if status2 == 200 {
                log::info!("Registration successful (MD5 auth)");
                *self.registered.lock().await = true;
                return Ok(true);
            }
            log::error!("Auth registration failed (status={})", status2);
            *self.registered.lock().await = false;
            return Ok(false);
        }

        if status == 200 {
            log::info!("Registration successful");
            *self.registered.lock().await = true;
            return Ok(true);
        }

        log::error!("Registration failed (status={})", status);
        *self.registered.lock().await = false;
        Ok(false)
    }

    /// UNREGISTER with the server (REGISTER with Expires: 0). Returns true on success.
    /// Notifies the registrar to immediately expire our registration binding.
    pub async fn unregister(&self) -> Result<bool> {
        let local = self.local_addr_str();
        let branch = self.new_branch();
        let call_id = self.new_call_id();
        let cseq = self.next_cseq().await;

        let mut settings = self.settings.clone();
        settings.register_expiry = 0; // set expiry to 0 to unregister

        let (msg, _cid, _cs) = build_register(
            &self.username,
            &self.domain,
            &local,
            &self.local_tag,
            &branch,
            &call_id,
            cseq,
            &settings,
            self.transport.via_str(),
        );

        let resp = self.send(&msg).await?;
        let status = utils::parse_status_code(&resp)?;

        // Handle MD5 Auth challenge on unregistration
        if (status == 401 || status == 407) && self.auth_method == crate::sip::AuthMethod::Md5 {
            let challenge = utils::extract_auth_challenge(&resp)
                .context("Cannot extract WWW-Authenticate params")?;

            let auth_msg = build_register_with_auth(
                &self.username,
                &self.password,
                &self.domain,
                &local,
                &self.local_tag,
                &self.new_branch(),
                // Reuse original Call-ID for auth retry (RFC 3261 §22.4)
                &call_id,
                self.next_cseq().await,
                &challenge,
                &settings,
                self.transport.via_str(),
            );

            let mut resp2 = self.send(&auth_msg).await?;
            let mut status2 = utils::parse_status_code(&resp2)?;

            if let Some(fresh) = stale_retry_challenge(status2, &resp2) {
                log::info!("Unregistration nonce was stale, retrying with the fresh one");
                let retry_msg = build_register_with_auth(
                    &self.username,
                    &self.password,
                    &self.domain,
                    &local,
                    &self.local_tag,
                    &self.new_branch(),
                    &call_id,
                    self.next_cseq().await,
                    &fresh,
                    &settings,
                    self.transport.via_str(),
                );
                resp2 = self.send(&retry_msg).await?;
                status2 = utils::parse_status_code(&resp2)?;
            }

            if status2 == 200 {
                log::info!("Unregistration successful (MD5 auth)");
                *self.registered.lock().await = false;
                return Ok(true);
            }
            log::error!("Auth unregistration failed (status={})", status2);
            return Ok(false);
        }

        if status == 200 {
            log::info!("Unregistration successful");
            *self.registered.lock().await = false;
            return Ok(true);
        }

        log::error!("Unregistration failed (status={})", status);
        Ok(false)
    }
}

/// The fresh challenge to retry with, when an authenticated request came back
/// challenged again only because the nonce had expired.
///
/// Returns None for any other rejection, so wrong credentials are not retried
/// in a loop.
pub(crate) fn stale_retry_challenge(status: u16, response: &str) -> Option<utils::AuthChallenge> {
    if status != 401 && status != 407 {
        return None;
    }
    utils::extract_auth_challenge(response).filter(|c| c.stale)
}

#[cfg(test)]
mod tests {
    use super::stale_retry_challenge;

    const STALE: &str = "SIP/2.0 401 Unauthorized\r\n\
                         WWW-Authenticate: Digest realm=\"example.com\", nonce=\"fresh\", stale=true\r\n\r\n";
    const REJECTED: &str = "SIP/2.0 401 Unauthorized\r\n\
                            WWW-Authenticate: Digest realm=\"example.com\", nonce=\"fresh\"\r\n\r\n";

    #[test]
    fn retries_only_an_expired_nonce() {
        let fresh = stale_retry_challenge(401, STALE).expect("stale challenge should be retried");
        assert_eq!(fresh.nonce, "fresh");
        assert!(stale_retry_challenge(407, STALE).is_some());
    }

    /// Wrong credentials come back challenged too — retrying those would loop.
    #[test]
    fn does_not_retry_a_plain_rejection() {
        assert!(stale_retry_challenge(401, REJECTED).is_none());
        assert!(stale_retry_challenge(403, STALE).is_none());
        assert!(stale_retry_challenge(200, STALE).is_none());
    }
}
