//! Call and registration watchers.
//!
//! Spawns and manages background tasks for handling registration status
//! and incoming INVITE call requests.

mod incoming_call;
mod registration;

pub use incoming_call::{incoming_call_watcher, parse_sdp_connection};
pub use registration::registration_watcher;
