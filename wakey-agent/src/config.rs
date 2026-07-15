use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub const DEFAULT_CONFIG_PATH: &str = "/etc/wakey-agent/config.toml";
pub const DEFAULT_PID_FILE: &str = "/var/run/wakey-agent.pid";
const WAKEY_DHCP_LEASES_ENV: &str = "WAKEY_DHCP_LEASES";
const WAKEY_MAC_NAME_CACHE_ENV: &str = "WAKEY_MAC_NAME_CACHE";
const WAKEY_OBSERVATION_STORE_ENV: &str = "WAKEY_OBSERVATION_STORE";
const DEFAULT_DHCP_LEASES_PATH: &str = "/tmp/dhcp.leases";
const DEFAULT_MAC_NAME_CACHE_PATH: &str = "/tmp/wakey_mac_names.json";
const DEFAULT_OBSERVATION_STORE_PATH: &str = "/tmp/wakey_observations.json";
pub const DEFAULT_OBSERVATION_RETENTION_DAYS: u64 = 7;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentConfig {
    pub server_url: String,
    pub agent_id: String,
    pub agent_token: String,
    #[serde(default = "default_reconnect_base_ms")]
    pub reconnect_base_ms: u64,
    #[serde(default = "default_reconnect_max_ms")]
    pub reconnect_max_ms: u64,
    #[serde(default = "default_observation_sync_interval_seconds")]
    pub observation_sync_interval_seconds: u64,
    #[serde(default = "default_observation_retention_days")]
    pub observation_retention_days: u64,
    #[serde(default = "default_pid_file")]
    pub pid_file: PathBuf,
    #[serde(default = "default_dhcp_leases_path")]
    pub dhcp_leases_path: PathBuf,
    #[serde(default = "default_mac_name_cache_path")]
    pub mac_name_cache_path: PathBuf,
    #[serde(default = "default_observation_store_path")]
    pub observation_store_path: PathBuf,
    #[serde(default)]
    pub terminal: TerminalConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_terminal_shell")]
    pub shell: PathBuf,
    #[serde(default = "default_terminal_max_sessions")]
    pub max_sessions: usize,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            shell: default_terminal_shell(),
            max_sessions: default_terminal_max_sessions(),
        }
    }
}

pub static DEFAULT_CONFIG: LazyLock<AgentConfig> = LazyLock::new(|| AgentConfig {
    server_url: "https://wakey.ldlda.com".to_string(),
    agent_id: "REPLACE_ME_AGENT_ID".to_string(),
    agent_token: "REPLACE_ME_AGENT_TOKEN".to_string(),
    reconnect_base_ms: default_reconnect_base_ms(),
    reconnect_max_ms: default_reconnect_max_ms(),
    observation_sync_interval_seconds: default_observation_sync_interval_seconds(),
    observation_retention_days: default_observation_retention_days(),
    pid_file: default_pid_file(),
    dhcp_leases_path: default_dhcp_leases_path(),
    mac_name_cache_path: default_mac_name_cache_path(),
    observation_store_path: default_observation_store_path(),
    terminal: TerminalConfig::default(),
});

impl fmt::Debug for AgentConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentConfig")
            .field("server_url", &self.server_url)
            .field("agent_id", &self.agent_id)
            .field("agent_token", &"<redacted>")
            .field("reconnect_base_ms", &self.reconnect_base_ms)
            .field("reconnect_max_ms", &self.reconnect_max_ms)
            .field(
                "observation_sync_interval_seconds",
                &self.observation_sync_interval_seconds,
            )
            .field(
                "observation_retention_days",
                &self.observation_retention_days,
            )
            .field("pid_file", &self.pid_file)
            .field("dhcp_leases_path", &self.dhcp_leases_path)
            .field("mac_name_cache_path", &self.mac_name_cache_path)
            .field("observation_store_path", &self.observation_store_path)
            .field("terminal", &self.terminal)
            .finish()
    }
}

const fn default_reconnect_base_ms() -> u64 {
    1_000
}

const fn default_reconnect_max_ms() -> u64 {
    30_000
}

const fn default_observation_sync_interval_seconds() -> u64 {
    60
}

const fn default_observation_retention_days() -> u64 {
    DEFAULT_OBSERVATION_RETENTION_DAYS
}

fn default_pid_file() -> PathBuf {
    DEFAULT_PID_FILE.into()
}

fn default_dhcp_leases_path() -> PathBuf {
    DEFAULT_DHCP_LEASES_PATH.into()
}

fn default_mac_name_cache_path() -> PathBuf {
    DEFAULT_MAC_NAME_CACHE_PATH.into()
}

fn default_observation_store_path() -> PathBuf {
    DEFAULT_OBSERVATION_STORE_PATH.into()
}

fn default_terminal_shell() -> PathBuf {
    "/bin/ash".into()
}

const fn default_terminal_max_sessions() -> usize {
    2
}

impl AgentConfig {
    pub fn local_path_envs(&self) -> Vec<(&'static str, &Path)> {
        vec![
            (WAKEY_DHCP_LEASES_ENV, self.dhcp_leases_path.as_path()),
            (WAKEY_MAC_NAME_CACHE_ENV, self.mac_name_cache_path.as_path()),
            (
                WAKEY_OBSERVATION_STORE_ENV,
                self.observation_store_path.as_path(),
            ),
        ]
    }
}

pub fn apply_local_path_env_to_command(cmd: &mut std::process::Command, config: &AgentConfig) {
    for (key, path) in config.local_path_envs() {
        cmd.env(key, path);
    }
}

pub fn load_config(path: &Path) -> Result<AgentConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read agent config {}", path.display()))?;
    toml::from_str(&content)
        .with_context(|| format!("failed to parse agent config {}", path.display()))
}

pub fn save_config(path: &Path, config: &AgentConfig) -> Result<()> {
    let content = toml::to_string_pretty(config).context("failed to serialize agent config")?;
    write_config_atomically(path, &content)
}

pub fn save_config_with_backup(path: &Path, config: &AgentConfig) -> Result<Option<PathBuf>> {
    let backup = if path.exists() {
        Some(snapshot_existing_config(path)?)
    } else {
        None
    };
    save_config(path, config)?;
    Ok(backup)
}

pub fn restore_backup(path: &Path, backup_path: &Path) -> Result<()> {
    let content = std::fs::read_to_string(backup_path)
        .with_context(|| format!("failed to read config backup {}", backup_path.display()))?;
    write_config_atomically(path, &content)
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

fn write_config_atomically(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config dir {}", parent.display()))?;
    }

    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, content)
        .with_context(|| format!("failed to write temp config {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| {
        format!(
            "failed to move temp config {} into {}",
            tmp.display(),
            path.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip() {
        let dir = std::env::temp_dir().join(format!("wakey-agent-config-{}", std::process::id()));
        let path = dir.join("config.toml");
        let config = AgentConfig {
            server_url: "https://example.com".into(),
            agent_id: "agent-1".into(),
            agent_token: "secret".into(),
            reconnect_base_ms: 123,
            reconnect_max_ms: 456,
            observation_sync_interval_seconds: 7,
            observation_retention_days: 3,
            pid_file: "/tmp/test-wakey-agent.pid".into(),
            dhcp_leases_path: "/tmp/test-dhcp.leases".into(),
            mac_name_cache_path: "/tmp/test-names.json".into(),
            observation_store_path: "/tmp/test-observations.json".into(),
            terminal: TerminalConfig {
                enabled: true,
                shell: "/bin/sh".into(),
                max_sessions: 2,
            },
        };

        save_config(&path, &config).expect("save");
        let loaded = load_config(&path).expect("load");
        assert_eq!(loaded, config);
        assert!(format!("{:?}", loaded).contains("<redacted>"));
        assert!(
            loaded
                .local_path_envs()
                .iter()
                .any(|(key, _)| *key == WAKEY_OBSERVATION_STORE_ENV)
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_without_retention_uses_current_default() {
        let config: AgentConfig = toml::from_str(
            r#"
server_url = "https://example.com"
agent_id = "agent-1"
agent_token = "secret"
"#,
        )
        .expect("config should parse");

        assert_eq!(
            config.observation_retention_days,
            DEFAULT_OBSERVATION_RETENTION_DAYS
        );
        assert_eq!(config.terminal, TerminalConfig::default());
    }
}
