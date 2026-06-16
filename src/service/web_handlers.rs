//! Web Dashboard API handlers registration.
//!
//! Submodules implement REST endpoints and WebSocket handlers for managing accounts,
//! calls, system status, log streaming, and server configuration.

pub mod accounts;
pub mod audio_ws;
pub mod auth;
pub mod calls;
pub mod config_handlers;
pub mod status;

pub use accounts::{
    add_account, delete_account, edit_account, get_accounts, register_account, unregister_account,
};
pub use audio_ws::audio_ws_handler;
pub use auth::login;
pub use calls::{
    call_account, dtmf_account, hangup_account, hold_account, play_account, resume_account,
    transfer_account,
};
pub use config_handlers::{get_config, put_config};
pub use status::{get_logs, get_status};
