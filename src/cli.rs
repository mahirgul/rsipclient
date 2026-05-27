//! CLI argument definitions using clap

use crate::ipc::DEFAULT_CONTROL_PORT;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "sip-client", version = "0.4.0")]
pub struct Cli {
    /// Path to TOML config file
    #[arg(short = 'c', long, default_value = "config.toml")]
    pub config: String,

    /// Account name to use (if omitted, first account is used)
    #[arg(short = 'a', long)]
    pub account: Option<String>,

    /// Override SIP server address (host:port) - direct mode only
    #[arg(short = 's', long)]
    pub server: Option<String>,

    /// Control port for service communication
    #[arg(long, default_value_t = DEFAULT_CONTROL_PORT)]
    pub ctrl_port: u16,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start the SIP service daemon (listens for commands)
    Service {
        /// Control port override
        #[arg(long, default_value_t = DEFAULT_CONTROL_PORT)]
        ctrl_port: u16,
    },

    /// Register an account with its SIP server
    Register,

    /// Make a call (INVITE)
    Call {
        /// Target SIP URI, e.g. sip:bob@example.com
        #[arg(short = 't', long)]
        target: String,
    },

    /// End current call (BYE)
    Hangup,

    /// Cancel an ongoing INVITE
    Cancel,

    /// Show status of all accounts on the running service
    Status,

    /// Shutdown the running service
    Shutdown,

    /// List all configured accounts (offline - just reads config)
    List,

    /// Play a WAV file over RTP during an active call
    Play {
        /// Account with active call
        #[arg(short = 'a', long)]
        account: String,

        /// Path to WAV file (8kHz, 16-bit mono PCM)
        #[arg(short = 'f', long)]
        file: String,
    },
}
