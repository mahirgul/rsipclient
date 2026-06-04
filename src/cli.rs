//! CLI argument definitions using clap

use crate::ipc::DEFAULT_CONTROL_PORT;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "sip-client", version = env!("CARGO_PKG_VERSION"))]
pub struct Cli {
    /// Path to TOML config file
    #[arg(short = 'c', long, default_value = "config.toml", global = true)]
    pub config: String,

    /// Account name to use (if omitted, first account is used)
    #[arg(short = 'a', long, global = true)]
    pub account: Option<String>,

    /// Override SIP server address (host:port) - direct mode only
    #[arg(short = 's', long, global = true)]
    pub server: Option<String>,

    /// Control port for service communication
    #[arg(long, default_value_t = DEFAULT_CONTROL_PORT, global = true)]
    pub ctrl_port: u16,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
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

    /// Put current call on hold (sendonly)
    Hold,

    /// Resume current call from hold (sendrecv)
    Resume,

    /// Transfer current call to another target (REFER)
    Transfer {
        /// Target SIP URI to transfer to, e.g. sip:operator@example.com
        #[arg(short = 't', long)]
        target: String,
    },

    /// Send DTMF digits (RFC 2833 telephone-event)
    Dtmf {
        /// DTMF digits to send, e.g. "1234#*"
        #[arg(short = 'd', long)]
        digits: String,
    },

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
