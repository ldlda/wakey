use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};

pub const DEFAULT_STATE_FILE: &str = "/var/lib/wakey-control-plane/state.json";
pub const DEFAULT_PID_FILE: &str = "/var/run/wakey-control-plane.pid";

#[derive(Parser)]
#[command(name = "wakey-control-plane")]
#[command(version, about = "Control plane server for wakey-agent fleets")]
pub struct Cli {
    #[arg(short = 'v', long = "verbose", action = ArgAction::Count, global = true)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run the control-plane daemon.
    Serve(ServeArgs),
    /// Create a new enroll token for provisioning a router.
    IssueEnrollToken(IssueEnrollTokenArgs),
    /// Send SIGHUP to an already-running daemon.
    Reload(ReloadArgs),
}

#[derive(Args, Clone)]
pub struct ServeArgs {
    #[arg(long, default_value = "0.0.0.0:8080")]
    pub bind: SocketAddr,

    #[arg(long, default_value = "http://127.0.0.1:8080")]
    pub public_url: String,

    #[arg(long, default_value = DEFAULT_STATE_FILE)]
    pub state_file: PathBuf,

    #[arg(long = "enroll-token")]
    pub enroll_tokens: Vec<String>,

    #[arg(long, default_value_t = 30_000)]
    pub command_timeout_ms: u64,

    #[arg(long, default_value = DEFAULT_PID_FILE)]
    pub pid_file: PathBuf,
}

#[derive(Args)]
pub struct IssueEnrollTokenArgs {
    #[arg(long, default_value = DEFAULT_STATE_FILE)]
    pub state_file: PathBuf,

    #[arg(long = "enroll-token")]
    pub enroll_tokens: Vec<String>,

    #[arg(long)]
    pub public_url: Option<String>,
}

#[derive(Args)]
pub struct ReloadArgs {
    #[arg(long, default_value = DEFAULT_PID_FILE)]
    pub pid_file: PathBuf,
}
