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

    // Only serve exports telemetry. Init-config keeps its output independent of logging setup.
    let _local_telemetry = if matches!(&cli.command, Command::Serve(_) | Command::InitConfig(_)) {
        None
    } else {
        Some(tracing::init(
            cli.verbose,
            &config::TelemetryConfig::default(),
        )?)
    };

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
            let telemetry = tracing::init(cli.verbose, &daemon.telemetry)?;
            let result = runtime::serve(daemon).await;
            tokio::task::spawn_blocking(move || telemetry.shutdown()).await?;
            result
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
        Command::IssueEnrollToken(args) => runtime::issue_enroll_token(args).await,
        Command::ListEnrollTokens(args) => runtime::list_enroll_tokens(args).await,
        Command::RevokeEnrollToken(args) => runtime::revoke_enroll_token(args).await,
        Command::RevokeAgent(args) => runtime::revoke_agent(args).await,
        Command::StateStats(args) => runtime::state_stats(args).await,
        Command::MigrateSqliteState(args) => runtime::migrate_sqlite_state(args).await,
        Command::Reload(args) => runtime::reload_daemon(&args.pid_file),
    }
}
