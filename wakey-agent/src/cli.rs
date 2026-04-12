use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};

use crate::config;

pub const DEFAULT_PID_FILE: &str = "/var/run/wakey-agent.pid";

#[derive(Parser)]
#[command(name = "wakey-agent")]
#[command(version, about = "Outbound control-plane agent for Wakey")]
pub struct Cli {
    /// Increase log verbosity. Use `-v` for debug and `-vv` for trace.
    #[arg(short = 'v', long = "verbose", action = ArgAction::Count, global = true)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start the agent daemon and maintain the outbound control-plane session.
    Serve(ServeArgs),
    /// Enroll this router with a control plane and write agent config.
    Enroll(EnrollArgs),
    /// Create a local config scaffold for manual bootstrap.
    InitConfig(InitConfigArgs),
    /// Reload a running agent daemon by sending SIGHUP.
    Reload(ReloadArgs),
}

#[derive(Args)]
pub struct ServeArgs {
    /// Path to the agent config file.
    #[arg(long, default_value = config::DEFAULT_CONFIG_PATH)]
    pub config: PathBuf,

    /// Path to pid file for reload signaling.
    #[arg(long, default_value = DEFAULT_PID_FILE)]
    pub pid_file: PathBuf,
}

#[derive(Args)]
pub struct EnrollArgs {
    /// Base HTTPS URL of the control plane.
    #[arg(long)]
    pub server_url: String,
    /// One-time or short-lived enroll token provided by the control plane.
    #[arg(long)]
    pub enroll_token: String,
    /// Path to the agent config file to write (overwritten on successful enroll).
    #[arg(long, default_value = config::DEFAULT_CONFIG_PATH)]
    pub config: PathBuf,

    /// Reload a running daemon after writing config so the new config takes effect immediately.
    #[arg(long)]
    pub reload_running: bool,

    /// Path to pid file used when `--reload-running` is enabled.
    #[arg(long, default_value = DEFAULT_PID_FILE)]
    pub pid_file: PathBuf,
}

#[derive(Args)]
pub struct InitConfigArgs {
    /// Path to the agent config file to write. If omitted, config is printed to stdout.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Print config to stdout.
    #[arg(long, conflicts_with = "config")]
    pub stdout: bool,

    /// Base HTTPS URL of the control plane.
    #[arg(long)]
    pub server_url: Option<String>,

    /// Persistent agent id obtained from enroll flow.
    #[arg(long)]
    pub agent_id: Option<String>,

    /// Persistent agent token obtained from enroll flow.
    #[arg(long)]
    pub agent_token: Option<String>,

    /// Replace an existing config file.
    #[arg(long)]
    pub force: bool,
}

#[derive(Args)]
pub struct ReloadArgs {
    /// Path to pid file for reload signaling.
    #[arg(long, default_value = DEFAULT_PID_FILE)]
    pub pid_file: PathBuf,
}
