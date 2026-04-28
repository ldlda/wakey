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
use cli::{Cli, Command, InitConfigArgs, ObserveCommand};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    tracing::init(cli.verbose);
    let global_config = cli.config.as_deref();

    match cli.command {
        Command::Serve(mut args) => {
            if let Some(config) = global_config {
                args.config = config.to_path_buf();
            }
            ::tracing::info!("wakey-agent command: serve");
            serve::serve(args).await?
        }
        Command::Enroll(mut args) => {
            if let Some(config) = global_config {
                args.config = config.to_path_buf();
            }
            let resolved_server_url = if let Some(server_url) = args.server_url.as_deref() {
                server_url.to_string()
            } else {
                match config::load_config(&args.config) {
                    Ok(cfg) => cfg.server_url,
                    Err(_) => anyhow::bail!(
                        "missing control-plane URL: pass --server-url or provide server_url in {}",
                        args.config.display()
                    ),
                }
            };

            ::tracing::info!(server_url = %resolved_server_url, config = %args.config.display(), "wakey-agent command: enroll");
            let outcome =
                enroll::enroll(&resolved_server_url, &args.enroll_token, &args.config).await?;
            println!("agent_id={}", outcome.config.agent_id);
            println!("config={}", args.config.display());
            println!("config_write=updated");
            if let Some(backup_path) = &outcome.backup_path {
                println!("config_backup={}", backup_path.display());
            }
            if args.reload_running {
                match serve::reload_daemon(&args.pid_file) {
                    Ok(()) => println!("reload=signaled pid_file={}", args.pid_file.display()),
                    Err(err) => {
                        ::tracing::warn!(error = %err, pid_file = %args.pid_file.display(), "enroll completed but daemon reload failed");
                        if let Some(backup_path) = &outcome.backup_path {
                            match config::restore_backup(&args.config, backup_path) {
                                Ok(()) => {
                                    println!("rollback=restored backup={}", backup_path.display());
                                    anyhow::bail!(
                                        "reload failed after enroll; restored previous config from {}",
                                        backup_path.display()
                                    );
                                }
                                Err(restore_err) => {
                                    anyhow::bail!(
                                        "reload failed after enroll and rollback failed: reload_error={}, rollback_error={}",
                                        err,
                                        restore_err
                                    );
                                }
                            }
                        } else {
                            anyhow::bail!(
                                "reload failed after enroll; no previous config backup was available"
                            );
                        }
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
        Command::InitConfig(mut args) => {
            if let Some(config) = global_config
                && args.config.is_none()
                && !args.stdout
            {
                args.config = Some(config.to_path_buf());
            }
            init_config(args)?
        }
        Command::Reload(args) => {
            ::tracing::info!(pid_file = %args.pid_file.display(), "wakey-agent command: reload");
            serve::reload_daemon(&args.pid_file)?
        }
        Command::SyncObservations(mut args) => {
            if let Some(config) = global_config {
                args.config = config.to_path_buf();
            }
            ::tracing::info!(config = %args.config.display(), "wakey-agent command: sync-observations");
            let cfg = config::load_config(&args.config)?;
            let accepted = session::sync_observations_once(&cfg).await?;
            println!("observations_synced={accepted}");
        }
        Command::Observe(mut args) => {
            if let Some(config) = global_config {
                args.config = config.to_path_buf();
            }
            observe(args)?
        }
    }

    Ok(())
}

fn observe(args: cli::ObserveArgs) -> Result<()> {
    let mut cmd = std::process::Command::new(resolve_wakey_binary());
    cmd.arg("observe");
    let config_path = args.config.clone();
    if let Ok(config) = config::load_config(&args.config) {
        config::apply_local_path_env_to_command(&mut cmd, &config);
    }

    match args.command {
        ObserveCommand::Dhcp(args) => {
            ::tracing::debug!(
                action = %args.action,
                mac = %args.mac,
                ip = ?args.ip,
                hostname = ?args.hostname,
                config = %config_path.display(),
                "forwarding dhcp observation to wakey"
            );
            cmd.arg("dhcp")
                .arg("--action")
                .arg(args.action)
                .arg("--mac")
                .arg(args.mac);
            if let Some(ip) = args.ip {
                cmd.arg("--ip").arg(ip);
            }
            if let Some(hostname) = args.hostname {
                cmd.arg("--hostname").arg(hostname);
            }
        }
        ObserveCommand::Neigh(args) => {
            ::tracing::debug!(
                action = %args.action,
                mac = ?args.mac,
                ip = ?args.ip,
                config = %config_path.display(),
                "forwarding neighbor observation to wakey"
            );
            cmd.arg("neigh").arg("--action").arg(args.action);
            if let Some(mac) = args.mac {
                cmd.arg("--mac").arg(mac);
            }
            if let Some(ip) = args.ip {
                cmd.arg("--ip").arg(ip);
            }
        }
    }

    let status = cmd.status()?;
    ::tracing::debug!(%status, "wakey observe exited");
    if !status.success() {
        anyhow::bail!("wakey observe exited with {status}");
    }
    Ok(())
}

fn resolve_wakey_binary() -> std::path::PathBuf {
    if let Ok(current) = std::env::current_exe()
        && let Some(dir) = current.parent()
    {
        let sibling = dir.join("wakey");
        if sibling.exists() {
            return sibling;
        }
    }

    let root_bin = std::path::PathBuf::from("/root/.bin/wakey");
    if root_bin.exists() {
        return root_bin;
    }

    "wakey".into()
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

    let mut cfg = if let Some(from_config) = &args.from_config {
        config::load_config(from_config)?
    } else {
        config::AgentConfig {
            server_url: "https://wakey.ldlda.com".to_string(),
            agent_id: "REPLACE_ME_AGENT_ID".to_string(),
            agent_token: "REPLACE_ME_AGENT_TOKEN".to_string(),
            reconnect_base_ms: 1_000,
            reconnect_max_ms: 30_000,
            observation_sync_interval_seconds: 60,
            dhcp_leases_path: "/tmp/dhcp.leases".into(),
            mac_name_cache_path: "/tmp/wakey_mac_names.json".into(),
            observation_store_path: "/tmp/wakey_observations.json".into(),
        }
    };

    if let Some(server_url) = args.server_url {
        cfg.server_url = server_url;
    }
    if let Some(agent_id) = args.agent_id {
        cfg.agent_id = agent_id;
    }
    if let Some(agent_token) = args.agent_token {
        cfg.agent_token = agent_token;
    }

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
