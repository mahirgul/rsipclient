//! IVR (Interactive Voice Response) system module.
//!
//! Handles auto-attendant menus, parsing account DTMF navigation,
//! and executing playback/record/transfer actions inside IVR call sessions.

#![allow(unused_imports)]

pub mod parser;
pub mod session;
pub mod types;

pub use parser::{build_ivr_config, parse_action, parse_menu};
pub use session::IvrSession;
pub use types::{IvrAction, IvrConfig, IvrMenu};
