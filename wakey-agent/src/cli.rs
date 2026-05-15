use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};

use crate::config;

#[derive(Parser)]
#[command(name = "wakey-agent")]
#[command(version, about = "Outbound control-plane agent for Wakey")]
pub struct Cli {
    /// Increase log verbosity. Use `-v` for debug and `-vv` for trace.
    #[arg(short = 'v', long = "verbose", action = ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Path to the agent config file for commands that use agent config.
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

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
    /// Pass local hotplug observations through to the wakey CLI.
    Observe(ObserveArgs),
}

#[derive(Args)]
pub struct ServeArgs {
    /// Path to the agent config file.
    #[arg(long, short = 'c', default_value = config::DEFAULT_CONFIG_PATH)]
    pub config: PathBuf,

    /// Path to pid file. Overrides `pid_file` in config.
    #[arg(long)]
    pub pid_file: Option<PathBuf>,
}

#[derive(Args)]
pub struct EnrollArgs {
    /// Base HTTPS URL of the control plane.
    ///
    /// Resolution order:
    /// 1) `--server-url` flag
    /// 2) existing `server_url` value in `--config`
    ///
    /// If neither source is available, enroll exits with an error.
    #[arg(long)]
    pub server_url: Option<String>,
    /// One-time or short-lived enroll token provided by the control plane.
    #[arg(long, short = 't', visible_alias = "token")]
    pub enroll_token: String,
    /// Path to the agent config file to write (overwritten on successful enroll).
    #[arg(long, default_value = config::DEFAULT_CONFIG_PATH)]
    pub config: PathBuf,

    /// Reload a running daemon after writing config so the new config takes effect immediately.
    #[arg(long)]
    pub reload_running: bool,

    /// Path to pid file used when `--reload-running` is enabled. Overrides `pid_file` in config.
    #[arg(long)]
    pub pid_file: Option<PathBuf>,
}

#[derive(Args)]
pub struct InitConfigArgs {
    /// Path to the agent config file to write. If omitted, config is printed to stdout.
    #[arg(long, short, visible_alias = "to", visible_short_alias = 't')]
    pub config: Option<PathBuf>,

    /// Existing agent config to use as a base before applying explicit overrides.
    #[arg(long, short = 'f', visible_alias = "from")]
    pub from_config: Option<PathBuf>,

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

    /// Days to keep local hook observation rows since last seen. Zero disables pruning.
    #[arg(long)]
    pub observation_retention_days: Option<u64>,

    /// Replace an existing config file.
    #[arg(long)]
    pub force: bool,
}

#[derive(Args)]
pub struct ReloadArgs {
    /// Path to pid file for reload signaling. Overrides `pid_file` in config.
    #[arg(long)]
    pub pid_file: Option<PathBuf>,
}

#[derive(Args)]
pub struct ObserveArgs {
    /// Path to the agent config file. If present, local path settings are passed through.
    #[arg(long, short, global = true, default_value = config::DEFAULT_CONFIG_PATH)]
    pub config: PathBuf,

    #[command(subcommand)]
    pub command: ObserveCommand,
}

#[derive(Subcommand)]
pub enum ObserveCommand {
    /// Observe a dnsmasq DHCP lease event.
    Dhcp(ObserveDhcpArgs),
    /// Observe a neighbor-table event.
    Neigh(ObserveNeighArgs),
}

#[derive(Args)]
pub struct ObserveDhcpArgs {
    #[arg(long)]
    pub action: String,
    #[arg(long)]
    pub mac: String,
    #[arg(long)]
    pub ip: Option<String>,
    #[arg(long)]
    pub hostname: Option<String>,
}

#[derive(Args)]
pub struct ObserveNeighArgs {
    #[arg(long)]
    pub action: String,
    #[arg(long)]
    pub mac: Option<String>,
    #[arg(long)]
    pub ip: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn global_config_is_accepted_before_enroll() {
        let cli = Cli::try_parse_from([
            "wakey-agent",
            "--config",
            "/tmp/wakey-agent.toml",
            "enroll",
            "--server-url",
            "https://wakey.example.com",
            "--enroll-token",
            "token-123",
        ])
        .expect("global --config should parse before enroll");

        assert_eq!(cli.config, Some(PathBuf::from("/tmp/wakey-agent.toml")));
        let Command::Enroll(args) = cli.command else {
            panic!("expected enroll command");
        };
        assert_eq!(
            args.server_url.as_deref(),
            Some("https://wakey.example.com")
        );
        assert_eq!(args.enroll_token, "token-123");
    }
}
