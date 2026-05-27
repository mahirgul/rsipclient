//! SIP Client module - voice over IP signaling
//!
//! Submodules:
//! - `auth`       : MD5 Digest authentication
//! - `client`     : SipClient struct + low-level helpers
//! - `messages`   : SIP request builders
//! - `operations` : register, invite, bye, cancel implementations
//! - `sdp`        : SDP body builder
//! - `settings`   : Per-account SIP settings
//! - `transport`  : UDP transport layer
//! - `utils`      : SIP header parsers, ID generation

pub mod auth;
pub mod client;
pub mod messages;
pub mod operations;
pub mod sdp;
pub mod settings;
pub mod transfer;
pub mod transport;
pub mod utils;

// Re-exports
pub use client::{AuthMethod, SipClient};
pub use settings::SipSettings;
