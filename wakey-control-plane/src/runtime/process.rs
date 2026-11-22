use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::{info, warn};

pub fn reload_daemon(pid_file: &Path) -> Result<()> {
    let pid = read_pid(pid_file)?;
    info!(pid, pid_file = %pid_file.display(), "sending control-plane reload signal");
    send_hup(pid)
}

pub(super) struct PidFile(PathBuf);

impl PidFile {
    pub(super) fn write(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create pid dir {}", parent.display()))?;
        }
        std::fs::write(path, format!("{}\n", std::process::id()))
            .with_context(|| format!("failed to write pid file {}", path.display()))?;
        Ok(Self(path.to_path_buf()))
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        if let Err(err) = std::fs::remove_file(&self.0)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            warn!(error = %err, pid_file = %self.0.display(), "failed to remove control-plane pid file");
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
    fn pid_file_is_removed_when_guard_is_dropped() {
        let dir = std::env::temp_dir().join(format!("wakey-cc-pid-{}", uuid::Uuid::new_v4()));
        let path = dir.join("wakey-cc.pid");
        let pid_file = PidFile::write(&path).unwrap();
        assert_eq!(read_pid(&path).unwrap() as u32, std::process::id());

        drop(pid_file);
        assert!(!path.exists());
        std::fs::remove_dir(dir).unwrap();
    }
}
