use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use wakey_core::config_duration::{self, LegacyDuration};
pub use wakey_core::telemetry::TelemetryConfig;

#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub data_dir: PathBuf,
    pub bind: SocketAddr,
    pub public_url: String,
    pub state_file: PathBuf,
    pub command_timeout: Duration,
    pub enroll_token_ttl: Duration,
    pub history_retention: Duration,
    pub pid_file: PathBuf,
    pub ui_dist_dir: PathBuf,
    pub bootstrap_enroll_tokens: Vec<String>,
    pub telemetry: TelemetryConfig,
}

/// TOML DTO for both loading partial files and generating complete files.
/// Options preserve omission for precedence; runtime conversion stays separate.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(remote = "Self")]
pub(crate) struct FileConfig {
    pub(crate) data_dir: Option<PathBuf>,
    pub(crate) bind: Option<String>,
    pub(crate) public_url: Option<String>,
    pub(crate) state_file: Option<PathBuf>,
    #[serde(default, with = "humantime_serde::option")]
    pub(crate) command_timeout: Option<Duration>,
    #[serde(default, with = "humantime_serde::option")]
    pub(crate) enroll_token_ttl: Option<Duration>,
    #[serde(default, with = "humantime_serde::option")]
    pub(crate) history_retention: Option<Duration>,
    pub(crate) pid_file: Option<PathBuf>,
    pub(crate) ui_dist_dir: Option<PathBuf>,
    #[serde(alias = "enroll_tokens")]
    pub(crate) bootstrap_enroll_tokens: Option<Vec<String>>,
    pub(crate) telemetry: Option<FileTelemetryConfig>,
}

pub(crate) const DURATION_KEYS: &[LegacyDuration] = &[
    LegacyDuration {
        old: "command_timeout_ms",
        new: "command_timeout",
        unit: Duration::from_millis(1),
    },
    LegacyDuration {
        old: "enroll_token_ttl_seconds",
        new: "enroll_token_ttl",
        unit: Duration::from_secs(1),
    },
    LegacyDuration {
        old: "observation_retention_seconds",
        new: "history_retention",
        unit: Duration::from_secs(1),
    },
];

impl Serialize for FileConfig {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Self::serialize(self, serializer)
    }
}
impl<'de> Deserialize<'de> for FileConfig {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        config_duration::deserialize_legacy(deserializer, DURATION_KEYS, Self::deserialize)
    }
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
    history_retention: Duration::from_secs(2_592_000),
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
            history_retention: Some(std::time::Duration::ZERO),
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

    #[test]
    fn legacy_durations_match_new_keys_and_emit_only_strings() {
        let legacy: FileConfig = toml::from_str("command_timeout_ms = 1234\nenroll_token_ttl_seconds = 3600\nobservation_retention_seconds = 2592000").unwrap();
        let canonical: FileConfig = toml::from_str(
            "command_timeout = '1234ms'\nenroll_token_ttl = '1h'\nhistory_retention = '30d'",
        )
        .unwrap();
        assert_eq!(legacy, canonical);
        let output = toml::to_string(&legacy).unwrap();
        for key in super::DURATION_KEYS {
            assert!(!output.contains(key.old));
            assert!(output.contains(key.new));
        }
        assert_eq!(toml::from_str::<FileConfig>(&output).unwrap(), canonical);
    }

    #[test]
    fn file_durations_reject_duplicates_and_bare_numbers() {
        for key in super::DURATION_KEYS {
            let both = format!("{} = '1s'\n{} = 1", key.new, key.old);
            assert!(toml::from_str::<FileConfig>(&both).is_err());
            let number = format!("{} = 1", key.new);
            assert!(toml::from_str::<FileConfig>(&number).is_err());
            let negative = format!("{} = -1", key.old);
            assert!(toml::from_str::<FileConfig>(&negative).is_err());
        }
    }
}
