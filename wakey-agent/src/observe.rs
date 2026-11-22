use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};

use crate::cli::{ObserveArgs, ObserveCommand};
use crate::config;

pub(crate) fn run(args: ObserveArgs) -> Result<()> {
    let binary = resolve_wakey_binary();
    let mut cmd = Command::new(&binary);
    cmd.arg("observe");
    let config_path = args.config;
    if let Some(config) = config::load_optional_config(&config_path)? {
        cmd.envs(config.local_path_envs());
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

    let status = cmd
        .status()
        .with_context(|| format!("failed to run {} observe", binary.display()))?;
    ::tracing::debug!(%status, "wakey observe exited");
    if !status.success() {
        anyhow::bail!("wakey observe exited with {status}");
    }
    Ok(())
}

fn resolve_wakey_binary() -> PathBuf {
    if let Ok(current) = std::env::current_exe()
        && let Some(dir) = current.parent()
    {
        let sibling = dir.join("wakey");
        if sibling.exists() {
            return sibling;
        }
    }

    let root_bin = PathBuf::from("/root/.bin/wakey");
    if root_bin.exists() {
        return root_bin;
    }

    "wakey".into()
}
