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
use cli::{Cli, Command, InitConfigArgs};

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
            if args.reload_running {
                match serve::reload_daemon(&args.pid_file) {
                    Ok(()) => println!("reload=signaled pid_file={}", args.pid_file.display()),
                    Err(err) => ::tracing::warn!(error = %err, pid_file = %args.pid_file.display(), "enroll completed but daemon reload failed"),
                }
            }
        }
        Command::InitConfig(args) => init_config(args)?,
        Command::Reload(args) => serve::reload_daemon(&args.pid_file)?,
    }

    Ok(())
}

fn init_config(args: InitConfigArgs) -> Result<()> {
    if args.config.exists() && !args.force {
        anyhow::bail!(
            "config {} already exists; re-run with --force to overwrite",
            args.config.display()
        );
    }

    let cfg = config::AgentConfig {
        server_url: args
            .server_url
            .unwrap_or_else(|| "https://control-plane.example.com".to_string()),
        agent_id: args.agent_id.unwrap_or_else(|| "REPLACE_ME_AGENT_ID".to_string()),
        agent_token: args
            .agent_token
            .unwrap_or_else(|| "REPLACE_ME_AGENT_TOKEN".to_string()),
        reconnect_base_ms: 1_000,
        reconnect_max_ms: 30_000,
    };

    config::save_config(&args.config, &cfg)?;
    println!("config={}", args.config.display());
    println!("next=wakey-agent serve --config {}", args.config.display());
    Ok(())
}
