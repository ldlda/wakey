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
            let existing_config = match config::load_config(&args.config) {
                Ok(cfg) => Some(cfg),
                Err(e) => {
                    if e.root_cause()
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound)
                    {
                        None
                    } else {
                        return Err(e);
                    }
                }
            };
            let resolved_server_url = if let Some(server_url) = args.server_url.as_deref() {
                server_url.to_string()
            } else if let Some(cfg) = existing_config.as_ref() {
                cfg.server_url.clone()
            } else {
                anyhow::bail!(
                    "missing control-plane URL: pass --server-url or provide server_url in {}",
                    args.config.display()
                );
            };

            ::tracing::info!(server_url = %resolved_server_url, config = %args.config.display(), "wakey-agent command: enroll");
            let outcome = enroll::enroll(
                &resolved_server_url,
                &args.enroll_token,
                &args.config,
                existing_config.as_ref(),
            )
            .await?;
            let pid_file = args
                .pid_file
                .as_deref()
                .unwrap_or(outcome.config.pid_file.as_path());
            println!("agent_id={}", outcome.config.agent_id);
            println!("config={}", args.config.display());
            println!("config_write=updated");
            if let Some(backup_path) = &outcome.backup_path {
                println!("config_backup={}", backup_path.display());
            }
            if args.reload_running {
                match serve::reload_daemon(pid_file) {
                    Ok(()) => println!("reload=signaled pid_file={}", pid_file.display()),
                    Err(err) => {
                        ::tracing::warn!(error = %err, pid_file = %pid_file.display(), "enroll completed but daemon reload failed");
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
                    "next=wakey-agent reload --pid-file {}  # if daemon is already running",
                    pid_file.display()
                );
                println!("run={}", serve_command_for_config(&args.config));
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
            let pid_file = resolve_pid_file(global_config, args.pid_file.as_deref())?;
            ::tracing::info!(pid_file = %pid_file.display(), "wakey-agent command: reload");
            serve::reload_daemon(&pid_file)?
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

fn serve_command_for_config(config: &std::path::Path) -> String {
    if config == std::path::Path::new(config::DEFAULT_CONFIG_PATH) {
        "wakey-agent serve".to_string()
    } else {
        format!("wakey-agent --config {} serve", config.display())
    }
}

fn resolve_pid_file(
    config_path: Option<&std::path::Path>,
    explicit_pid_file: Option<&std::path::Path>,
) -> Result<std::path::PathBuf> {
    if let Some(pid_file) = explicit_pid_file {
        return Ok(pid_file.to_path_buf());
    }
    if let Some(config_path) = config_path {
        return Ok(config::load_config(config_path)?.pid_file);
    }
    Ok(config::DEFAULT_PID_FILE.into())
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
            observation_retention_days: config::DEFAULT_OBSERVATION_RETENTION_DAYS,
            pid_file: config::DEFAULT_PID_FILE.into(),
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
    if let Some(days) = args.observation_retention_days {
        cfg.observation_retention_days = days;
    }

    if let Some(path) = &args.config {
        config::save_config(path, &cfg)?;
        println!("config={}", path.display());
        println!("run={}", serve_command_for_config(path));
    } else {
        let rendered = toml::to_string_pretty(&cfg)?;
        print!("{}", rendered);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serve_command_omits_default_config_path() {
        assert_eq!(
            serve_command_for_config(std::path::Path::new(config::DEFAULT_CONFIG_PATH)),
            "wakey-agent serve"
        );
    }

    #[test]
    fn serve_command_includes_custom_config_path() {
        assert_eq!(
            serve_command_for_config(std::path::Path::new("/tmp/wakey-agent.toml")),
            "wakey-agent --config /tmp/wakey-agent.toml serve"
        );
    }

    #[test]
    fn explicit_pid_file_wins_without_loading_config() {
        let pid_file = resolve_pid_file(
            Some(std::path::Path::new("/tmp/missing-agent.toml")),
            Some(std::path::Path::new("/tmp/explicit.pid")),
        )
        .expect("explicit pid should resolve");

        assert_eq!(pid_file, std::path::PathBuf::from("/tmp/explicit.pid"));
    }

    #[test]
    fn pid_file_defaults_when_no_config_is_available() {
        let pid_file = resolve_pid_file(None, None).expect("default pid should resolve");

        assert_eq!(pid_file, std::path::PathBuf::from(config::DEFAULT_PID_FILE));
    }
}
