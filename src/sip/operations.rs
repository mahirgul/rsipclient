//! SIP operations module.
//!
//! This module aggregates and re-exports SIP operations (registration, call control, and media management)
//! implemented as extension methods on `SipClient` across separate files to ensure clean and readable code structure.

mod call;
mod hold_transfer;
mod register;
