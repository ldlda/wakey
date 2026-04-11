use std::path::Path;

use anyhow::{Context, Result};
use tracing::info;

pub fn reload_daemon(pid_file: &Path) -> Result<()> {
    let pid = read_pid(pid_file)?;
    info!(pid, pid_file = %pid_file.display(), "sending control-plane reload signal");
    send_hup(pid)
}

pub fn write_pid_file(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create pid dir {}", parent.display()))?;
    }
    std::fs::write(path, format!("{}\n", std::process::id()))
        .with_context(|| format!("failed to write pid file {}", path.display()))
}

pub fn remove_pid_file(path: &Path) -> Result<()> {
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
