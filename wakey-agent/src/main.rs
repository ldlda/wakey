mod cli;
mod config;
mod dispatch;
mod enroll;
mod observe;
mod ping;
mod serve;
mod session;
mod terminal;
mod tracing;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};
use wakey_agent::protocol;

#[tokio::main]
async fn main() -> Result<()> {
    let Cli {
        verbose,
        config,
        command,
    } = Cli::parse();

    // Only the daemon starts OTLP exporters. Short-lived commands log locally.
    let _local_telemetry = if matches!(&command, Command::Serve(_)) {
        None
    } else {
        Some(tracing::init(verbose, &Default::default(), None)?)
    };

    match command {
        Command::Serve(mut args) => {
            args.config = config.unwrap_or(args.config);
            serve::run(args, verbose).await
        }
        Command::Enroll(mut args) => {
            args.config = config.unwrap_or(args.config);
            enroll::run(args).await
        }
        Command::InitConfig(mut args) => {
            if !args.stdout {
                args.config = args.config.or(config);
            }
            config::init(args)
        }
        Command::Reload(args) => serve::reload(config.as_deref(), args),
        Command::Observe(mut args) => {
            args.config = config.unwrap_or(args.config);
            observe::run(args)
        }
    }
}
