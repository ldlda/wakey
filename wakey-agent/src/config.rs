use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const DEFAULT_CONFIG_PATH: &str = "/etc/wakey-agent/config.toml";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentConfig {
    pub server_url: String,
    pub agent_id: String,
    pub agent_token: String,
    #[serde(default = "default_reconnect_base_ms")]
    pub reconnect_base_ms: u64,
    #[serde(default = "default_reconnect_max_ms")]
    pub reconnect_max_ms: u64,
}

const fn default_reconnect_base_ms() -> u64 {
    1_000
}

const fn default_reconnect_max_ms() -> u64 {
    30_000
}

pub fn load_config(path: &Path) -> Result<AgentConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read agent config {}", path.display()))?;
    toml::from_str(&content)
        .with_context(|| format!("failed to parse agent config {}", path.display()))
}

pub fn save_config(path: &Path, config: &AgentConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config dir {}", parent.display()))?;
    }

    let content = toml::to_string_pretty(config).context("failed to serialize agent config")?;
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
        };

        save_config(&path, &config).expect("save");
        let loaded = load_config(&path).expect("load");
        assert_eq!(loaded, config);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
