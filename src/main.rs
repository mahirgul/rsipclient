//! SIP CLI Client - Service & IPC Edition
#![allow(clippy::too_many_arguments)]
//!
//! Service mode:
//!   cargo run -- -c config.toml service
//!
//! CLI mode (sends command to running service):
//!   cargo run -- -c config.toml register -a alice
//!   cargo run -- -c config.toml call -a bob -t sip:charlie@example.com
//!   cargo run -- -c config.toml play -a alice -f audio.wav
//!   cargo run -- -c config.toml status
//!   cargo run -- -c config.toml shutdown

mod cli;
mod config;
mod ipc;
mod ipc_client;
mod ivr;
mod rtp;
mod service;
mod sip;

use anyhow::{Context, Result};
use clap::Parser;
use cli::{Cli, Command};
use config::Config;
use ipc::Request;

#[tokio::main]
async fn main() -> Result<()> {
    service::logger::init_logger();
    let cli = Cli::parse();

    // --- Service mode ---
    if let Some(Command::Service { ctrl_port }) = cli.command {
        println!("Loading config: {}", cli.config);
        let cfg = Config::load(&cli.config).context("Failed to load config file")?;
        let svc = service::Service::new(&cfg, ctrl_port, cli.config.clone())
            .await
            .context("Failed to create service")?;
        svc.run().await?;
        return Ok(());
    }

    // --- Offline list ---
    if let Some(Command::List) = cli.command {
        let cfg = Config::load(&cli.config)?;
        println!("Configured accounts:");
        for (i, a) in cfg.accounts.iter().enumerate() {
            let disp = a.display_name.as_deref().unwrap_or("-");
            let asserted = a.asserted_id.as_deref().unwrap_or("-");
            let proxy = a.proxy.as_deref().unwrap_or("-");
            println!(
                "  {}. {}  {}@{}  display:\"{}\"  srv:{}  proxy:{}  SIP:{}  RTP:{}-{}  auth:{}  codec:{}  asserted:{}  expiry:{}s",
                i + 1,
                a.name,
                a.username,
                a.domain,
                disp,
                a.server,
                proxy,
                if a.sip_port == 0 { "auto".to_string() } else { a.sip_port.to_string() },
                a.rtp_port_start,
                a.rtp_port_end,
                a.auth_method.as_deref().unwrap_or("md5"),
                a.codec.as_deref().unwrap_or("pcmu"),
                asserted,
                a.register_expiry.unwrap_or(3600),
            );
        }
        return Ok(());
    }

    // --- IPC mode (register, call, hangup, cancel, status, shutdown, play) ---
    let Some(ref cmd) = cli.command else {
        Cli::parse_from(["", "--help"]);
        return Ok(());
    };

    let (cmd_str, account, target_opt) = match cmd {
        Command::Register => ("register", cli.account.clone(), None),
        Command::Call { target } => ("call", cli.account.clone(), Some(target.clone())),
        Command::Hangup => ("hangup", cli.account.clone(), None),
        Command::Cancel => ("cancel", cli.account.clone(), None),
        Command::Hold => ("hold", cli.account.clone(), None),
        Command::Resume => ("resume", cli.account.clone(), None),
        Command::Transfer { target } => ("transfer", cli.account.clone(), Some(target.clone())),
        Command::Dtmf { digits } => ("dtmf", cli.account.clone(), Some(digits.clone())),
        Command::Status => ("status", None, None),
        Command::Shutdown => ("shutdown", None, None),
        Command::Play { account, file } => ("play", Some(account.clone()), Some(file.clone())),
        _ => unreachable!(),
    };

    let req = match (cmd_str, account, target_opt.clone()) {
        ("status", _, _) => Request::new("status"),
        ("shutdown", _, _) => Request::new("shutdown"),
        (c, Some(a), Some(t)) => Request::with_target(c, &a, &t),
        (c, Some(a), None) => Request::with_account(c, &a),
        (c, None, _) => {
            let cfg = Config::load(&cli.config)?;
            let default_account = &cfg.accounts[0].name;
            eprintln!("Note: No account specified, using '{}'", default_account);
            if let Some(t) = target_opt {
                Request::with_target(c, default_account, &t)
            } else {
                Request::with_account(c, default_account)
            }
        }
    };

    let resp = ipc_client::send_ipc(&req, cli.ctrl_port).await?;
    if resp.ok {
        println!("OK: {}", resp.msg);
    } else {
        eprintln!("FAIL: {}", resp.msg);
        std::process::exit(1);
    }

    Ok(())
}
