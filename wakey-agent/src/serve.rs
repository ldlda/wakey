use std::path::Path;

use anyhow::{Context, Result};
use tracing::{info, warn};

use crate::cli::ServeArgs;
use crate::{config, session};

pub async fn serve(args: ServeArgs) -> Result<()> {
    if !args.config.exists() {
        anyhow::bail!(
            "agent config {} not found. Run `wakey-agent enroll --server-url <url> --enroll-token <token>` or `wakey-agent init-config --config {}` first",
            args.config.display(),
            args.config.display()
        );
    }

    write_pid_file(&args.pid_file)?;

    let mut cfg = config::load_config(&args.config)?;
    info!(config_path = %args.config.display(), agent_id = %cfg.agent_id, "starting wakey-agent");

    let mut worker = tokio::spawn(session::run(cfg.clone()));

    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut hup = signal(SignalKind::hangup()).context("failed to install SIGHUP handler")?;

        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    info!("ctrl-c received; shutting down wakey-agent");
                    worker.abort();
                    break;
                }
                _ = hup.recv() => {
                    match config::load_config(&args.config) {
                        Ok(new_cfg) => {
                            cfg = new_cfg;
                            info!(config_path = %args.config.display(), agent_id = %cfg.agent_id, "reload requested; restarting session with updated config");
                            worker.abort();
                            worker = tokio::spawn(session::run(cfg.clone()));
                        }
                        Err(err) => {
                            warn!(error = %err, config_path = %args.config.display(), "reload requested but config reload failed; keeping current session");
                        }
                    }
                }
                join = &mut worker => {
                    let _ = remove_pid_file(&args.pid_file);
                    return join.context("agent session join failed")?;
                }
            }
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .context("failed waiting for ctrl-c")?;
        worker.abort();
    }

    let _ = remove_pid_file(&args.pid_file);
    Ok(())
}

pub fn reload_daemon(pid_file: &Path) -> Result<()> {
    let pid = read_pid(pid_file)?;
    send_hup(pid)
}

fn write_pid_file(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create pid dir {}", parent.display()))?;
    }
    std::fs::write(path, format!("{}\n", std::process::id()))
        .with_context(|| format!("failed to write pid file {}", path.display()))
}

fn remove_pid_file(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => {
            Err(err).with_context(|| format!("failed to remove pid file {}", path.display()))
        }
    }
}

fn read_pid(path: &Path) -> Result<i32> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read pid file {}", path.display()))?;
    let pid = raw
        .trim()
        .parse::<i32>()
        .with_context(|| format!("invalid pid in {}", path.display()))?;
    if pid <= 0 {
        anyhow::bail!("invalid non-positive pid {pid}");
    }
    Ok(pid)
}

fn send_hup(pid: i32) -> Result<()> {
    let status = std::process::Command::new("kill")
        .arg("-HUP")
        .arg(pid.to_string())
        .status()
        .context("failed to invoke kill -HUP")?;
    if !status.success() {
        anyhow::bail!("kill -HUP failed for pid {pid}");
    }
    Ok(())
}
