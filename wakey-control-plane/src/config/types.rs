use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
pub use wakey_core::telemetry::TelemetryConfig;

#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub data_dir: PathBuf,
    pub bind: SocketAddr,
    pub public_url: String,
    pub state_file: PathBuf,
    pub command_timeout: Duration,
    pub enroll_token_ttl: Duration,
    pub observation_retention: Duration,
    pub pid_file: PathBuf,
    pub ui_dist_dir: PathBuf,
    pub bootstrap_enroll_tokens: Vec<String>,
    pub telemetry: TelemetryConfig,
}

/// TOML DTO for both loading partial files and generating complete files.
/// Options preserve omission for precedence; runtime conversion stays separate.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub(crate) struct FileConfig {
    pub(crate) data_dir: Option<PathBuf>,
    pub(crate) bind: Option<String>,
    pub(crate) public_url: Option<String>,
    pub(crate) state_file: Option<PathBuf>,
    pub(crate) command_timeout_ms: Option<u64>,
    pub(crate) enroll_token_ttl_seconds: Option<u64>,
    pub(crate) observation_retention_seconds: Option<u64>,
    pub(crate) pid_file: Option<PathBuf>,
    pub(crate) ui_dist_dir: Option<PathBuf>,
    #[serde(alias = "enroll_tokens")]
    pub(crate) bootstrap_enroll_tokens: Option<Vec<String>>,
    pub(crate) telemetry: Option<FileTelemetryConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub(crate) struct FileTelemetryConfig {
    // Missing fields must remain distinguishable from explicit values when
    // init-config merges an existing file with CLI overrides and defaults.
    pub(crate) console_filter: Option<String>,
    pub(crate) otlp_log_filter: Option<String>,
    pub(crate) trace_filter: Option<String>,
    pub(crate) otlp_endpoint: Option<String>,
    pub(crate) authorization_env: Option<String>,
    pub(crate) service_name: Option<String>,
    pub(crate) json_logs: Option<bool>,
}

// Relative template paths are resolved under data_dir by the runtime loader.
pub(crate) static DEFAULT_CONFIG: LazyLock<DaemonConfig> = LazyLock::new(|| DaemonConfig {
    data_dir: crate::cli::DEFAULT_DATA_DIR.into(),
    bind: SocketAddr::from(([0, 0, 0, 0], 8080)),
    public_url: "http://127.0.0.1:8080".to_string(),
    state_file: "state.sqlite3".into(),
    command_timeout: Duration::from_millis(30_000),
    enroll_token_ttl: Duration::from_secs(86_400),
    observation_retention: Duration::from_secs(2_592_000),
    pid_file: "wakey-control-plane.pid".into(),
    ui_dist_dir: "ui/dist".into(),
    bootstrap_enroll_tokens: Vec::new(),
    telemetry: TelemetryConfig {
        console_filter: None,
        otlp_log_filter: None,
        trace_filter: None,
        otlp_endpoint: None,
        authorization_env: None,
        service_name: "wakey-control-plane".to_string(),
        json_logs: false,
    },
});

pub struct IssueTokenSettings {
    pub data_dir: PathBuf,
    pub state_file: PathBuf,
    pub public_url: Option<String>,
    pub ttl: Duration,
}

pub struct StateAccessSettings {
    pub data_dir: PathBuf,
    pub state_file: PathBuf,
    pub public_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{FileConfig, FileTelemetryConfig};

    #[test]
    fn file_dto_round_trip_preserves_omission_and_explicit_values() {
        let omitted: FileConfig = toml::from_str("").unwrap();
        assert_eq!(omitted, FileConfig::default());
        assert_eq!(toml::to_string(&omitted).unwrap(), "");

        let explicit = FileConfig {
            bootstrap_enroll_tokens: Some(Vec::new()),
            observation_retention_seconds: Some(0),
            telemetry: Some(FileTelemetryConfig {
                json_logs: Some(false),
                console_filter: Some("info".into()),
                otlp_log_filter: Some("debug".into()),
                trace_filter: Some("info".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let rendered = toml::to_string_pretty(&explicit).unwrap();
        let restored: FileConfig = toml::from_str(&rendered).unwrap();
        assert_eq!(restored, explicit);
        assert_ne!(restored, omitted);
    }

    #[test]
    fn legacy_token_alias_serializes_with_canonical_name() {
        let file: FileConfig = toml::from_str("enroll_tokens = []").unwrap();
        assert_eq!(file.bootstrap_enroll_tokens, Some(Vec::new()));
        let rendered = toml::to_string(&file).unwrap();
        assert!(rendered.contains("bootstrap_enroll_tokens = []"));
        assert!(!rendered.starts_with("enroll_tokens"));
    }
}
