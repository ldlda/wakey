//! HTTP + WebSocket control plane: agent enrollment, registry, command relay, and operator UI hosting.

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
            if args.bootstrap_config {
                let created = config::bootstrap_config_if_missing(&args)?;
                if created {
                    eprintln!(
                        "bootstrapped missing config at {}",
                        args.config_file.display()
                    );
                }
            }
            let daemon = config::DaemonConfig::from_serve_args(&args)?;
            tracing::init(cli.verbose, &daemon.telemetry)?;
            runtime::serve(daemon).await
        }
        Command::InitConfig(args) => {
            if let Some(path) = config::write_init_config(&args)? {
                println!("config={}", path.display());
                println!(
                    "next=wakey-control-plane serve --config-file {}",
                    path.display()
                );
            }
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
        Command::RevokeAgent(args) => {
            tracing::init(cli.verbose, &config::TelemetryConfig::default())?;
            runtime::revoke_agent(args).await
        }
        Command::StateStats(args) => {
            tracing::init(cli.verbose, &config::TelemetryConfig::default())?;
            runtime::state_stats(args).await
        }
        Command::MigrateSqliteState(args) => {
            tracing::init(cli.verbose, &config::TelemetryConfig::default())?;
            runtime::migrate_sqlite_state(args).await
        }
        Command::Reload(args) => {
            tracing::init(cli.verbose, &config::TelemetryConfig::default())?;
            runtime::reload_daemon(&args.pid_file)
        }
    }
}
