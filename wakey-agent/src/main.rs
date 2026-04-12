mod cli;
mod config;
mod dispatch;
mod enroll;
mod protocol;
mod serve;
mod session;
mod tracing;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command, InitConfigArgs};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    tracing::init(cli.verbose);

    match cli.command {
        Command::Serve(args) => {
            ::tracing::info!("wakey-agent command: serve");
            serve::serve(args).await?
        }
        Command::Enroll(args) => {
            ::tracing::info!(server_url = %args.server_url, config = %args.config.display(), "wakey-agent command: enroll");
            let config = enroll::enroll(&args.server_url, &args.enroll_token, &args.config).await?;
            println!("agent_id={}", config.agent_id);
            println!("config={}", args.config.display());
            println!("config_write=updated");
            if args.reload_running {
                match serve::reload_daemon(&args.pid_file) {
                    Ok(()) => println!("reload=signaled pid_file={}", args.pid_file.display()),
                    Err(err) => {
                        ::tracing::warn!(error = %err, pid_file = %args.pid_file.display(), "enroll completed but daemon reload failed")
                    }
                }
            } else {
                println!("reload=not_requested");
                println!("runtime_config=unchanged_until_reload_or_restart");
                println!(
                    "next=wakey-agent reload --pid-file {}  # or restart wakey-agent",
                    args.pid_file.display()
                );
            }
        }
        Command::InitConfig(args) => init_config(args)?,
        Command::Reload(args) => {
            ::tracing::info!(pid_file = %args.pid_file.display(), "wakey-agent command: reload");
            serve::reload_daemon(&args.pid_file)?
        }
    }

    Ok(())
}

fn init_config(args: InitConfigArgs) -> Result<()> {
    if args.stdout && args.config.is_some() {
        anyhow::bail!("--stdout cannot be used with --config");
    }

    if let Some(config) = &args.config
        && config.exists()
        && !args.force
    {
        anyhow::bail!(
            "config {} already exists; re-run with --force to overwrite",
            config.display()
        );
    }

    let cfg = config::AgentConfig {
        server_url: args
            .server_url
            .unwrap_or_else(|| "https://control-plane.example.com".to_string()),
        agent_id: args
            .agent_id
            .unwrap_or_else(|| "REPLACE_ME_AGENT_ID".to_string()),
        agent_token: args
            .agent_token
            .unwrap_or_else(|| "REPLACE_ME_AGENT_TOKEN".to_string()),
        reconnect_base_ms: 1_000,
        reconnect_max_ms: 30_000,
    };

    if let Some(path) = &args.config {
        config::save_config(path, &cfg)?;
        println!("config={}", path.display());
        println!("next=wakey-agent serve --config {}", path.display());
    } else {
        let rendered = toml::to_string_pretty(&cfg)?;
        print!("{}", rendered);
    }
    Ok(())
}
