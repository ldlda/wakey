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
    tracing::init(cli.verbose);

    match cli.command {
        Command::Serve(args) => runtime::serve(args).await,
        Command::IssueEnrollToken(args) => runtime::issue_enroll_token(args).await,
        Command::Reload(args) => runtime::reload_daemon(&args.pid_file),
    }
}
