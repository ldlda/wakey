use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};

pub const DEFAULT_PID_FILE: &str = "/var/lib/wakey-control-plane/wakey-control-plane.pid";
pub const DEFAULT_DATA_DIR: &str = "/var/lib/wakey-control-plane";
pub const DEFAULT_CONFIG_FILE: &str = "/etc/wakey-control-plane/config.toml";

#[derive(Args, Clone, Copy, Default)]
pub struct AdminTargetArgs {
    #[arg(long, conflicts_with = "offline")]
    pub live: bool,

    #[arg(long, conflicts_with = "live")]
    pub offline: bool,
}

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
    /// Render a control-plane config scaffold.
    InitConfig(InitConfigArgs),
    /// Create a new enroll token for provisioning a router.
    IssueEnrollToken(IssueEnrollTokenArgs),
    /// List current enroll tokens and their expiration status.
    ListEnrollTokens(ListEnrollTokensArgs),
    /// Revoke a specific enroll token.
    RevokeEnrollToken(RevokeEnrollTokenArgs),
    /// Revoke an enrolled agent's persistent credentials.
    RevokeAgent(RevokeAgentArgs),
    /// Print state backend stats.
    StateStats(StateStatsArgs),
    /// Import a legacy sled state directory into a SQLite state file.
    ImportSledState(ImportSledStateArgs),
    /// Send SIGHUP to an already-running daemon.
    Reload(ReloadArgs),
}

#[derive(Args, Clone)]
pub struct ServeArgs {
    #[arg(long)]
    pub data_dir: Option<PathBuf>,

    #[arg(long)]
    pub bind: Option<SocketAddr>,

    #[arg(long)]
    pub public_url: Option<String>,

    #[arg(long)]
    pub state_file: Option<PathBuf>,

    #[arg(long = "bootstrap-enroll-token", visible_alias = "enroll-token")]
    pub bootstrap_enroll_tokens: Vec<String>,

    #[arg(long)]
    pub command_timeout_ms: Option<u64>,

    #[arg(long)]
    pub enroll_token_ttl_seconds: Option<u64>,

    #[arg(long)]
    pub pid_file: Option<PathBuf>,

    #[arg(long)]
    pub ui_dist_dir: Option<PathBuf>,

    #[arg(long, visible_alias="config", default_value = DEFAULT_CONFIG_FILE)]
    pub config_file: PathBuf,

    #[arg(long)]
    pub bootstrap_config: bool,
}

#[derive(Args, Clone)]
pub struct InitConfigArgs {
    #[arg(long = "config-file", alias = "config")]
    pub config_file: Option<PathBuf>,

    #[arg(long, conflicts_with = "config_file")]
    pub stdout: bool,

    #[arg(long)]
    pub data_dir: Option<PathBuf>,

    #[arg(long)]
    pub bind: Option<SocketAddr>,

    #[arg(long)]
    pub public_url: Option<String>,

    #[arg(long)]
    pub state_file: Option<PathBuf>,

    #[arg(long)]
    pub pid_file: Option<PathBuf>,

    #[arg(long)]
    pub ui_dist_dir: Option<PathBuf>,

    #[arg(long)]
    pub command_timeout_ms: Option<u64>,

    #[arg(long)]
    pub enroll_token_ttl_seconds: Option<u64>,

    #[arg(long = "bootstrap-enroll-token", visible_alias = "enroll-token")]
    pub bootstrap_enroll_tokens: Vec<String>,

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
    #[arg(long, default_value = DEFAULT_CONFIG_FILE)]
    pub config_file: PathBuf,

    #[arg(long)]
    pub state_file: Option<PathBuf>,

    #[arg(long)]
    pub data_dir: Option<PathBuf>,

    #[arg(long)]
    pub public_url: Option<String>,

    #[arg(long)]
    pub ttl_seconds: Option<u64>,

    #[command(flatten)]
    pub target: AdminTargetArgs,
}

#[derive(Args)]
pub struct ListEnrollTokensArgs {
    #[arg(long, default_value = DEFAULT_CONFIG_FILE)]
    pub config_file: PathBuf,

    #[arg(long)]
    pub state_file: Option<PathBuf>,

    #[arg(long)]
    pub data_dir: Option<PathBuf>,

    #[arg(long)]
    pub public_url: Option<String>,

    #[arg(long)]
    pub json: bool,

    #[command(flatten)]
    pub target: AdminTargetArgs,
}

#[derive(Args)]
pub struct RevokeEnrollTokenArgs {
    #[arg(long, default_value = DEFAULT_CONFIG_FILE)]
    pub config_file: PathBuf,

    #[arg(long)]
    pub state_file: Option<PathBuf>,

    #[arg(long)]
    pub data_dir: Option<PathBuf>,

    #[arg(long)]
    pub public_url: Option<String>,

    #[arg(long)]
    pub token: String,

    #[command(flatten)]
    pub target: AdminTargetArgs,
}

#[derive(Args)]
pub struct RevokeAgentArgs {
    #[arg(long, default_value = DEFAULT_CONFIG_FILE)]
    pub config_file: PathBuf,

    #[arg(long)]
    pub state_file: Option<PathBuf>,

    #[arg(long)]
    pub data_dir: Option<PathBuf>,

    #[arg(long)]
    pub public_url: Option<String>,

    #[arg(long)]
    pub agent_id: String,

    #[command(flatten)]
    pub target: AdminTargetArgs,
}

#[derive(Args)]
pub struct StateStatsArgs {
    #[arg(long, default_value = DEFAULT_CONFIG_FILE)]
    pub config_file: PathBuf,

    #[arg(long)]
    pub state_file: Option<PathBuf>,

    #[arg(long)]
    pub data_dir: Option<PathBuf>,

    #[arg(long)]
    pub public_url: Option<String>,

    #[arg(long)]
    pub json: bool,

    #[command(flatten)]
    pub target: AdminTargetArgs,
}

#[derive(Args)]
pub struct ImportSledStateArgs {
    #[arg(long)]
    pub from_sled_state: PathBuf,

    #[arg(long)]
    pub to_state_file: PathBuf,

    #[arg(long)]
    pub force: bool,
}

#[derive(Args)]
pub struct ReloadArgs {
    #[arg(long, default_value = DEFAULT_PID_FILE)]
    pub pid_file: PathBuf,
}
