mod api;
mod cli;
mod config;
mod runtime;
mod state;
mod tracing;
mod ws;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Serve(args) => {
            let daemon = config::DaemonConfig::from_serve_args(&args)?;
            tracing::init(cli.verbose, &daemon.telemetry)?;
            runtime::serve(daemon).await
        }
        Command::InitConfig(args) => {
            tracing::init(cli.verbose, &config::TelemetryConfig::default())?;
            config::write_init_config(&args)?;
            println!("config={}", args.config_file.display());
            println!(
                "next=wakey-control-plane serve --config-file {}",
                args.config_file.display()
            );
            Ok(())
        }
        Command::IssueEnrollToken(args) => {
            tracing::init(cli.verbose, &config::TelemetryConfig::default())?;
            runtime::issue_enroll_token(args).await
        }
        Command::ListEnrollTokens(args) => {
            tracing::init(cli.verbose, &config::TelemetryConfig::default())?;
            runtime::list_enroll_tokens(args).await
        }
        Command::RevokeEnrollToken(args) => {
            tracing::init(cli.verbose, &config::TelemetryConfig::default())?;
            runtime::revoke_enroll_token(args).await
        }
        Command::StateStats(args) => {
            tracing::init(cli.verbose, &config::TelemetryConfig::default())?;
            runtime::state_stats(args).await
        }
        Command::Reload(args) => {
            tracing::init(cli.verbose, &config::TelemetryConfig::default())?;
            runtime::reload_daemon(&args.pid_file)
        }
    }
}
