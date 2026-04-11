use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};

pub const DEFAULT_STATE_FILE: &str = "/var/lib/wakey-control-plane/state.db";
pub const DEFAULT_PID_FILE: &str = "/var/run/wakey-control-plane.pid";
pub const DEFAULT_CONFIG_FILE: &str = "/etc/wakey-control-plane/config.toml";

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
    /// Write a control-plane config scaffold.
    InitConfig(InitConfigArgs),
    /// Create a new enroll token for provisioning a router.
    IssueEnrollToken(IssueEnrollTokenArgs),
    /// Send SIGHUP to an already-running daemon.
    Reload(ReloadArgs),
}

#[derive(Args, Clone)]
pub struct ServeArgs {
    #[arg(long)]
    pub bind: Option<SocketAddr>,

    #[arg(long)]
    pub public_url: Option<String>,

    #[arg(long)]
    pub state_file: Option<PathBuf>,

    #[arg(long = "enroll-token")]
    pub enroll_tokens: Vec<String>,

    #[arg(long)]
    pub command_timeout_ms: Option<u64>,

    #[arg(long)]
    pub pid_file: Option<PathBuf>,

    #[arg(long, default_value = DEFAULT_CONFIG_FILE)]
    pub config_file: PathBuf,
}

#[derive(Args, Clone)]
pub struct InitConfigArgs {
    #[arg(long, default_value = DEFAULT_CONFIG_FILE)]
    pub config_file: PathBuf,

    #[arg(long)]
    pub bind: Option<SocketAddr>,

    #[arg(long)]
    pub public_url: Option<String>,

    #[arg(long)]
    pub state_file: Option<PathBuf>,

    #[arg(long)]
    pub pid_file: Option<PathBuf>,

    #[arg(long)]
    pub command_timeout_ms: Option<u64>,

    #[arg(long = "enroll-token")]
    pub enroll_tokens: Vec<String>,

    #[arg(long)]
    pub telemetry_otlp_endpoint: Option<String>,

    #[arg(long)]
    pub telemetry_service_name: Option<String>,

    #[arg(long)]
    pub telemetry_json_logs: bool,

    #[arg(long)]
    pub force: bool,
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
