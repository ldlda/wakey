use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result};

use super::AgentConfig;

#[derive(Clone, Copy)]
pub(super) enum Publish {
    Create,
    Replace,
}

pub(super) fn save_config(path: &Path, config: &AgentConfig, publish: Publish) -> Result<()> {
    config.validate()?;
    let content = toml::to_string_pretty(config).context("failed to serialize agent config")?;
    write_config_atomically(path, &content, publish)
}

pub fn save_config_with_backup(path: &Path, config: &AgentConfig) -> Result<Option<PathBuf>> {
    let backup = if path.exists() {
        Some(snapshot_existing_config(path)?)
    } else {
        None
    };
    save_config(path, config, Publish::Replace)?;
    Ok(backup)
}

pub fn restore_backup(path: &Path, backup_path: &Path) -> Result<()> {
    let content = std::fs::read_to_string(backup_path)
        .with_context(|| format!("failed to read config backup {}", backup_path.display()))?;
    write_config_atomically(path, &content, Publish::Replace)
}

fn snapshot_existing_config(path: &Path) -> Result<PathBuf> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config.toml");
    let backup_name = format!("{file_name}.bak.{ts}");
    let backup_path = path.with_file_name(backup_name);
    std::fs::copy(path, &backup_path).with_context(|| {
        format!(
            "failed to create config backup {} from {}",
            backup_path.display(),
            path.display()
        )
    })?;
    Ok(backup_path)
}

fn write_config_atomically(path: &Path, content: &str, publish: Publish) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create config dir {}", parent.display()))?;
    let (temp, mut file) = StagedConfig::create(parent)?;
    let written = file.write_all(content.as_bytes());
    // Close the handle before publishing or cleaning up the temp path.
    drop(file);
    written.with_context(|| format!("failed to write temp config {}", temp.0.display()))?;
    temp.publish(path, publish)
}

struct StagedConfig(PathBuf);

impl StagedConfig {
    fn create(parent: &Path) -> Result<(Self, File)> {
        static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        loop {
            let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(".wakey-config-{}-{suffix}.tmp", std::process::id()));
            match options.open(&path) {
                Ok(file) => return Ok((Self(path), file)),
                Err(err) if err.kind() == ErrorKind::AlreadyExists => continue,
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!("failed to create temp config {}", path.display())
                    });
                }
            }
        }
    }

    fn publish(self, path: &Path, mode: Publish) -> Result<()> {
        // A hard link publishes the complete file without replacing an existing path.
        let result = match mode {
            Publish::Create => match std::fs::hard_link(&self.0, path) {
                Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                    return Err(err).with_context(|| {
                        format!(
                            "config {} already exists; re-run with --force to overwrite",
                            path.display()
                        )
                    });
                }
                result => result,
            },
            Publish::Replace => std::fs::rename(&self.0, path),
        };
        result.with_context(|| format!("failed to publish config {}", path.display()))
    }
}

impl Drop for StagedConfig {
    fn drop(&mut self) {
        // Rename consumes the temp name; hard links and failed writes leave it behind.
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_config_creation_does_not_overwrite_the_winner() {
        let dir = std::env::temp_dir().join(format!("wakey-config-publish-{}", std::process::id()));
        let path = dir.join("config.toml");
        let path = path.as_path();
        let (first, second) = std::thread::scope(|scope| {
            let first = scope.spawn(|| write_config_atomically(path, "first", Publish::Create));
            let second = scope.spawn(|| write_config_atomically(path, "second", Publish::Create));
            (first.join().unwrap(), second.join().unwrap())
        });
        let (winner, error) = match (first, second) {
            (Ok(()), Err(error)) => ("first", error),
            (Err(error), Ok(())) => ("second", error),
            outcomes => panic!("expected exactly one config to be created: {outcomes:?}"),
        };
        assert!(error.to_string().contains("--force"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), winner);
        write_config_atomically(path, "replacement", Publish::Replace).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "replacement");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
