mod config;
mod dispatch;
mod enroll;
mod protocol;
mod session;
mod tracing;

use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};
use ::tracing::info;

#[derive(Parser)]
#[command(name = "wakey-agent")]
#[command(version, about = "Outbound control-plane agent for Wakey")]
struct Cli {
    /// Increase log verbosity. Use `-v` for debug and `-vv` for trace.
    #[arg(short = 'v', long = "verbose", action = ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start the agent daemon and maintain the outbound control-plane session.
    Serve(ServeArgs),
    /// Enroll this router with a control plane and write agent config.
    Enroll(EnrollArgs),
}

#[derive(Args)]
struct ServeArgs {
    /// Path to the agent config file.
    #[arg(long, default_value = config::DEFAULT_CONFIG_PATH)]
    config: PathBuf,
}

#[derive(Args)]
struct EnrollArgs {
    /// Base HTTPS URL of the control plane.
    #[arg(long)]
    server_url: String,
    /// One-time or short-lived enroll token provided by the control plane.
    #[arg(long)]
    enroll_token: String,
    /// Path to the agent config file to write.
    #[arg(long, default_value = config::DEFAULT_CONFIG_PATH)]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    tracing::init(cli.verbose);

    match cli.command {
        Command::Serve(args) => {
            let config = config::load_config(&args.config)?;
            info!(config_path = %args.config.display(), agent_id = %config.agent_id, "starting wakey-agent");
            session::run(config).await?;
        }
        Command::Enroll(args) => {
            let config = enroll::enroll(&args.server_url, &args.enroll_token, &args.config).await?;
            println!("agent_id={}", config.agent_id);
            println!("config={}", args.config.display());
        }
    }

    Ok(())
}
