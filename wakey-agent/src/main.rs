mod cli;
mod config;
mod dispatch;
mod enroll;
mod protocol;
mod session;
mod serve;
mod tracing;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    tracing::init(cli.verbose);

    match cli.command {
        Command::Serve(args) => serve::serve(args).await?,
        Command::Enroll(args) => {
            let config = enroll::enroll(&args.server_url, &args.enroll_token, &args.config).await?;
            println!("agent_id={}", config.agent_id);
            println!("config={}", args.config.display());
        }
        Command::Reload(args) => serve::reload_daemon(&args.pid_file)?,
    }

    Ok(())
}
