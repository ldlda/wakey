use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::{info, warn};

use crate::cli::{ReloadArgs, ServeArgs};
use crate::config::{self, AgentConfig};
use crate::session;

pub(crate) async fn run(args: ServeArgs, verbose: u8) -> Result<()> {
    let cfg = config::load_optional_config(&args.config)?.with_context(|| {
        format!(
            "agent config {} not found. Run `wakey-agent enroll --server-url <url> --enroll-token <token> --config {}` or `wakey-agent init-config --config {}` first",
            args.config.display(),
            args.config.display(),
            args.config.display()
        )
    })?;
    let telemetry = crate::tracing::init(verbose, &cfg.telemetry, Some(&cfg.agent_id))?;
    info!("wakey-agent command: serve");
    let result = serve(args, cfg).await;
    tokio::task::spawn_blocking(move || telemetry.shutdown()).await?;
    result
}

async fn serve(args: ServeArgs, mut cfg: AgentConfig) -> Result<()> {
    let pid_file = args.pid_file.unwrap_or_else(|| cfg.pid_file.clone());
    write_pid_file(&pid_file)?;
    info!(pid_file = %pid_file.display(), "wrote wakey-agent pid file");

    info!(config_path = %args.config.display(), agent_id = %cfg.agent_id, "starting wakey-agent");

    let mut worker = tokio::spawn(session::run(cfg.clone()));

    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut hup = signal(SignalKind::hangup()).context("failed to install SIGHUP handler")?;
        let mut terminate =
            signal(SignalKind::terminate()).context("failed to install SIGTERM handler")?;

        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    info!("ctrl-c received; shutting down wakey-agent");
                    worker.abort();
                    break;
                }
                _ = terminate.recv() => {
                    info!("SIGTERM received; shutting down wakey-agent");
                    worker.abort();
                    break;
                }
                _ = hup.recv() => {
                    match config::load_config(&args.config) {
                        Ok(new_cfg) => {
                            if new_cfg.telemetry != cfg.telemetry {
                                warn!("telemetry configuration changed; restart the daemon to apply it");
                            }
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
                    warn!("agent worker task exited; shutting down daemon");
                    let _ = remove_pid_file(&pid_file);
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

    // Await the cancelled session task before flushing its telemetry.
    let _ = worker.await;
    let _ = remove_pid_file(&pid_file);
    Ok(())
}

pub(crate) fn command_for_config(path: &Path) -> String {
    if path == Path::new(config::DEFAULT_CONFIG_PATH) {
        "wakey-agent serve".to_string()
    } else {
        format!("wakey-agent --config {} serve", path.display())
    }
}

pub(crate) fn reload(config_path: Option<&Path>, args: ReloadArgs) -> Result<()> {
    let pid_file = resolve_pid_file(config_path, args.pid_file.as_deref())?;
    info!(pid_file = %pid_file.display(), "wakey-agent command: reload");
    reload_daemon(&pid_file)
}

fn resolve_pid_file(
    config_path: Option<&Path>,
    explicit_pid_file: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(pid_file) = explicit_pid_file {
        return Ok(pid_file.to_path_buf());
    }
    if let Some(config_path) = config_path {
        return Ok(config::load_config(config_path)?.pid_file);
    }
    Ok(config::DEFAULT_PID_FILE.into())
}

pub(crate) fn reload_daemon(pid_file: &Path) -> Result<()> {
    let pid = read_pid(pid_file)?;
    info!(pid, pid_file = %pid_file.display(), "sending wakey-agent reload signal");
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
    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;

        kill(Pid::from_raw(pid), Signal::SIGHUP)
            .with_context(|| format!("failed to send SIGHUP to pid {pid}"))?;
        Ok(())
    }

    #[cfg(not(unix))]
    {
        let _ = pid;
        anyhow::bail!("reload is only supported on Unix (SIGHUP unavailable on this platform)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serve_command_omits_default_config_path() {
        assert_eq!(
            command_for_config(std::path::Path::new(config::DEFAULT_CONFIG_PATH)),
            "wakey-agent serve"
        );
    }

    #[test]
    fn serve_command_includes_custom_config_path() {
        assert_eq!(
            command_for_config(std::path::Path::new("/tmp/wakey-agent.toml")),
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
