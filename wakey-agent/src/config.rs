use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;
use wakey_core::config_duration::{self, LegacyDuration};

use crate::protocol::{
    DEFAULT_TERMINAL_MAX_SESSIONS, DEFAULT_TERMINAL_SESSION_TTL_SECONDS,
    checked_terminal_session_ttl,
};

pub const DEFAULT_CONFIG_PATH: &str = "/etc/wakey-agent/config.toml";
pub const DEFAULT_PID_FILE: &str = "/var/run/wakey-agent.pid";
const WAKEY_DHCP_LEASES_ENV: &str = "WAKEY_DHCP_LEASES";
const WAKEY_MAC_NAME_CACHE_ENV: &str = "WAKEY_MAC_NAME_CACHE";
const WAKEY_OBSERVATION_STORE_ENV: &str = "WAKEY_OBSERVATION_STORE";
const DEFAULT_DHCP_LEASES_PATH: &str = "/tmp/dhcp.leases";
const DEFAULT_MAC_NAME_CACHE_PATH: &str = "/tmp/wakey_mac_names.json";
const DEFAULT_OBSERVATION_STORE_PATH: &str = "/tmp/wakey_observations.json";
pub const DEFAULT_OBSERVATION_RETENTION_DAYS: u64 = 7;

// Derive canonical serde functions once; the trait adapter below translates
// legacy keys without maintaining another copy of every config field.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(remote = "Self")]
pub struct AgentConfig {
    pub server_url: String,
    pub agent_id: String,
    pub agent_token: String,
    #[serde(default = "default_reconnect_base", with = "humantime_serde")]
    pub reconnect_base: Duration,
    #[serde(default = "default_reconnect_max", with = "humantime_serde")]
    pub reconnect_max: Duration,
    #[serde(
        default = "default_observation_sync_interval",
        with = "humantime_serde"
    )]
    pub observation_sync_interval: Duration,
    #[serde(default = "default_observation_retention", with = "humantime_serde")]
    pub observation_retention: Duration,
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
    #[serde(default)]
    pub telemetry: wakey_core::telemetry::TelemetryConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct TerminalConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_terminal_shell")]
    pub shell: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub current_dir: Option<PathBuf>,
    #[serde(default = "default_terminal_max_sessions")]
    pub max_sessions: usize,
    /// Maximum lifetime of a PTY process. Zero disables automatic expiry.
    #[serde(default = "default_terminal_session_ttl", with = "humantime_serde")]
    pub session_ttl: Duration,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            shell: default_terminal_shell(),
            args: Vec::new(),
            current_dir: None,
            max_sessions: default_terminal_max_sessions(),
            session_ttl: default_terminal_session_ttl(),
        }
    }
}

pub static DEFAULT_CONFIG: LazyLock<AgentConfig> = LazyLock::new(|| AgentConfig {
    server_url: "https://wakey.ldlda.com".to_string(),
    agent_id: "REPLACE_ME_AGENT_ID".to_string(),
    agent_token: "REPLACE_ME_AGENT_TOKEN".to_string(),
    reconnect_base: default_reconnect_base(),
    reconnect_max: default_reconnect_max(),
    observation_sync_interval: default_observation_sync_interval(),
    observation_retention: default_observation_retention(),
    pid_file: default_pid_file(),
    dhcp_leases_path: default_dhcp_leases_path(),
    mac_name_cache_path: default_mac_name_cache_path(),
    observation_store_path: default_observation_store_path(),
    terminal: TerminalConfig::default(),
    telemetry: wakey_core::telemetry::TelemetryConfig::default(),
});

impl fmt::Debug for AgentConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentConfig")
            .field("server_url", &self.server_url)
            .field("agent_id", &self.agent_id)
            .field("agent_token", &"<redacted>")
            .field("reconnect_base", &self.reconnect_base)
            .field("reconnect_max", &self.reconnect_max)
            .field("observation_sync_interval", &self.observation_sync_interval)
            .field("observation_retention", &self.observation_retention)
            .field("pid_file", &self.pid_file)
            .field("dhcp_leases_path", &self.dhcp_leases_path)
            .field("mac_name_cache_path", &self.mac_name_cache_path)
            .field("observation_store_path", &self.observation_store_path)
            .field("terminal", &self.terminal)
            .field("telemetry", &self.telemetry)
            .finish()
    }
}

const fn default_reconnect_base() -> Duration {
    Duration::from_secs(1)
}

const fn default_reconnect_max() -> Duration {
    Duration::from_secs(30)
}

const fn default_observation_sync_interval() -> Duration {
    Duration::from_secs(60)
}

const fn default_observation_retention() -> Duration {
    Duration::from_secs(DEFAULT_OBSERVATION_RETENTION_DAYS * 86_400)
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
    "/bin/sh".into()
}

const fn default_terminal_max_sessions() -> usize {
    DEFAULT_TERMINAL_MAX_SESSIONS
}

const fn default_terminal_session_ttl() -> Duration {
    Duration::from_secs(DEFAULT_TERMINAL_SESSION_TTL_SECONDS)
}

impl TerminalConfig {
    pub(crate) fn session_ttl(&self) -> Result<Option<std::time::Duration>> {
        anyhow::ensure!(
            self.session_ttl.subsec_nanos() == 0,
            "terminal.session_ttl must use whole seconds (the terminal protocol uses seconds)"
        );
        checked_terminal_session_ttl(self.session_ttl.as_secs()).map_err(anyhow::Error::msg)
    }
}

impl AgentConfig {
    /// Validate scheduling bounds and second-based storage/protocol boundaries.
    /// Zero is allowed for hook retention and terminal TTL, not active timers.
    fn validate(&self) -> Result<()> {
        for (name, duration) in [
            ("reconnect_base", self.reconnect_base),
            ("reconnect_max", self.reconnect_max),
            ("observation_sync_interval", self.observation_sync_interval),
        ] {
            anyhow::ensure!(!duration.is_zero(), "{name} must be positive");
            anyhow::ensure!(
                std::time::Instant::now().checked_add(duration).is_some(),
                "{name} exceeds the platform timer range"
            );
        }
        anyhow::ensure!(
            self.observation_retention.subsec_nanos() == 0,
            "observation_retention must use whole seconds (hook timestamps use seconds)"
        );
        self.terminal.session_ttl()?;
        Ok(())
    }

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
    let raw: toml::Value = toml::from_str(&content)
        .with_context(|| format!("failed to parse agent config {}", path.display()))?;
    let mut value = serde_json::to_value(raw)?;
    let mut deprecated = config_duration::normalize(&mut value, AGENT_DURATION_KEYS)
        .map_err(anyhow::Error::msg)?
        .into_iter()
        .map(|key| ("", key))
        .collect::<Vec<_>>();
    if let Some(terminal) = value.get_mut("terminal") {
        deprecated.extend(
            config_duration::normalize(terminal, TERMINAL_DURATION_KEYS)
                .map_err(anyhow::Error::msg)?
                .into_iter()
                .map(|key| ("terminal.", key)),
        );
    }
    let config: AgentConfig = serde_json::from_value(value)
        .with_context(|| format!("failed to parse agent config {}", path.display()))?;
    config.validate()?;
    for (prefix, key) in deprecated {
        // Initial config loading precedes tracing setup; stderr also keeps
        // init-config stdout valid TOML and never prints config secrets.
        eprintln!(
            "warning: {}: `{prefix}{}` is deprecated; use `{prefix}{}` with a duration string",
            path.display(),
            key.old,
            key.new
        );
    }
    Ok(config)
}

const AGENT_DURATION_KEYS: &[LegacyDuration] = &[
    LegacyDuration {
        old: "reconnect_base_ms",
        new: "reconnect_base",
        unit: Duration::from_millis(1),
    },
    LegacyDuration {
        old: "reconnect_max_ms",
        new: "reconnect_max",
        unit: Duration::from_millis(1),
    },
    LegacyDuration {
        old: "observation_sync_interval_seconds",
        new: "observation_sync_interval",
        unit: Duration::from_secs(1),
    },
    LegacyDuration {
        old: "observation_retention_days",
        new: "observation_retention",
        unit: Duration::from_secs(86_400),
    },
];
const TERMINAL_DURATION_KEYS: &[LegacyDuration] = &[LegacyDuration {
    old: "session_ttl_seconds",
    new: "session_ttl",
    unit: Duration::from_secs(1),
}];

impl Serialize for AgentConfig {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        Self::serialize(self, serializer)
    }
}
impl<'de> Deserialize<'de> for AgentConfig {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        config_duration::deserialize_legacy(deserializer, AGENT_DURATION_KEYS, Self::deserialize)
    }
}
impl Serialize for TerminalConfig {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        Self::serialize(self, serializer)
    }
}
impl<'de> Deserialize<'de> for TerminalConfig {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        config_duration::deserialize_legacy(deserializer, TERMINAL_DURATION_KEYS, Self::deserialize)
    }
}

pub fn save_config(path: &Path, config: &AgentConfig) -> Result<()> {
    config.validate()?;
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

    const HEADER: &str =
        "server_url = 'https://example.com'\nagent_id = 'agent-1'\nagent_token = 'secret'\n";

    #[test]
    fn legacy_duration_keys_convert_and_serialize_canonically() {
        let legacy: AgentConfig = toml::from_str(&format!("{HEADER}\nreconnect_base_ms = 123\nreconnect_max_ms = 456\nobservation_sync_interval_seconds = 7\nobservation_retention_days = 3\n[terminal]\nsession_ttl_seconds = 600\n")).unwrap();
        let canonical: AgentConfig = toml::from_str(&format!("{HEADER}\nreconnect_base = '123ms'\nreconnect_max = '456ms'\nobservation_sync_interval = '7s'\nobservation_retention = '3d'\n[terminal]\nsession_ttl = '10m'\n")).unwrap();
        assert_eq!(legacy, canonical);
        legacy.validate().unwrap();
        let output = toml::to_string(&legacy).unwrap();
        for key in AGENT_DURATION_KEYS {
            assert!(!output.contains(key.old));
            assert!(output.contains(key.new));
        }
        assert!(!output.contains("session_ttl_seconds"));
        assert_eq!(toml::from_str::<AgentConfig>(&output).unwrap(), canonical);
    }

    #[test]
    fn config_rejects_ambiguous_units_and_invalid_timer_values() {
        for fields in [
            "reconnect_base = '1s'\nreconnect_base_ms = 1000",
            "observation_retention = '7d'\nobservation_retention_days = 7",
            "reconnect_base = 1000",
            "observation_retention_days = -1",
            "observation_retention_days = 9223372036854775807",
            "reconnect_base = 'nonsense'",
            "[terminal]\nsession_ttl = '1h'\nsession_ttl_seconds = 3600",
        ] {
            assert!(
                toml::from_str::<AgentConfig>(&format!("{HEADER}{fields}")).is_err(),
                "{fields}"
            );
        }
        for fields in [
            "observation_sync_interval = '0s'",
            "reconnect_base = '0s'",
            "[terminal]\nsession_ttl = '500ms'",
        ] {
            let config: AgentConfig = toml::from_str(&format!("{HEADER}{fields}")).unwrap();
            assert!(config.validate().is_err(), "{fields}");
        }
        let config: AgentConfig = toml::from_str(&format!(
            "{HEADER}observation_retention = '0d'\n[terminal]\nsession_ttl = '0s'"
        ))
        .unwrap();
        config.validate().unwrap();
        assert!(config.observation_retention.is_zero());
        assert_eq!(config.terminal.session_ttl().unwrap(), None);
        let oversized = AgentConfig {
            reconnect_max: Duration::from_secs(u64::MAX),
            ..config
        };
        assert!(oversized.validate().is_err());
    }

    #[test]
    fn config_roundtrip() {
        let dir = std::env::temp_dir().join(format!("wakey-agent-config-{}", std::process::id()));
        let path = dir.join("config.toml");
        let config = AgentConfig {
            server_url: "https://example.com".into(),
            agent_id: "agent-1".into(),
            agent_token: "secret".into(),
            reconnect_base: Duration::from_millis(123),
            reconnect_max: Duration::from_millis(456),
            observation_sync_interval: Duration::from_secs(7),
            observation_retention: Duration::from_secs(3 * 86_400),
            pid_file: "/tmp/test-wakey-agent.pid".into(),
            dhcp_leases_path: "/tmp/test-dhcp.leases".into(),
            mac_name_cache_path: "/tmp/test-names.json".into(),
            observation_store_path: "/tmp/test-observations.json".into(),
            telemetry: wakey_core::telemetry::TelemetryConfig {
                otlp_endpoint: Some("http://10.0.0.1:4317".into()),
                authorization_env: Some("WAKEY_OTLP_AUTHORIZATION".into()),
                service_name: "router-agent".into(),
                json_logs: true,
                console_filter: Some("info".into()),
                otlp_log_filter: Some("debug".into()),
                trace_filter: Some("info".into()),
            },
            terminal: TerminalConfig {
                enabled: true,
                shell: "/bin/sh".into(),
                args: vec!["-l".into()],
                current_dir: Some("/tmp".into()),
                max_sessions: 2,
                session_ttl: Duration::from_secs(600),
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
            config.observation_retention,
            DEFAULT_CONFIG.observation_retention
        );
        assert_eq!(config.terminal, TerminalConfig::default());
    }

    #[test]
    fn existing_terminal_config_defaults_optional_command_fields() {
        let config: AgentConfig = toml::from_str(
            r#"
server_url = "https://example.com"
agent_id = "agent-1"
agent_token = "secret"

[terminal]
enabled = true
shell = "/bin/sh"
max_sessions = 2
"#,
        )
        .expect("existing terminal config should parse");

        assert!(config.terminal.args.is_empty());
        assert!(config.terminal.current_dir.is_none());
        assert_eq!(
            config.terminal.session_ttl,
            TerminalConfig::default().session_ttl
        );
    }

    #[test]
    fn terminal_config_rejects_misspelled_fields() {
        let error = toml::from_str::<AgentConfig>(
            r#"
server_url = "https://example.com"
agent_id = "agent-1"
agent_token = "secret"

[terminal]
enabled = true
max_session = 67
"#,
        )
        .expect_err("unknown terminal fields must not silently use defaults");

        assert!(error.to_string().contains("max_session"));
    }

    #[test]
    fn terminal_ttl_zero_is_unlimited_and_positive_is_exact() {
        let mut terminal = TerminalConfig {
            session_ttl: Duration::ZERO,
            ..TerminalConfig::default()
        };
        assert_eq!(terminal.session_ttl().expect("unlimited TTL"), None);

        terminal.session_ttl = Duration::from_secs(67);
        assert_eq!(
            terminal.session_ttl().expect("finite TTL"),
            Some(std::time::Duration::from_secs(67))
        );

        terminal.session_ttl = Duration::from_secs(u64::MAX);
        assert!(terminal.session_ttl().is_err());
    }
}
