use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub data_dir: PathBuf,
    pub bind: SocketAddr,
    pub public_url: String,
    pub state_file: PathBuf,
    pub command_timeout: Duration,
    pub enroll_token_ttl: Duration,
    #[allow(dead_code)]
    pub observation_retention: Duration,
    pub pid_file: PathBuf,
    pub ui_dist_dir: PathBuf,
    pub bootstrap_enroll_tokens: Vec<String>,
    pub telemetry: TelemetryConfig,
}

#[derive(Debug, Clone)]
pub struct TelemetryConfig {
    pub otlp_endpoint: Option<String>,
    pub service_name: String,
    pub json_logs: bool,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        let defaults = &DEFAULT_CONFIG.telemetry;
        Self {
            otlp_endpoint: defaults.otlp_endpoint.clone(),
            service_name: defaults.service_name.clone(),
            json_logs: defaults.json_logs,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
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

#[derive(Debug, Deserialize, Default)]
pub(crate) struct FileTelemetryConfig {
    pub(crate) otlp_endpoint: Option<String>,
    pub(crate) service_name: Option<String>,
    pub(crate) json_logs: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct WritableConfig {
    pub(crate) data_dir: PathBuf,
    pub(crate) bind: String,
    pub(crate) public_url: String,
    pub(crate) state_file: PathBuf,
    pub(crate) command_timeout_ms: u64,
    pub(crate) enroll_token_ttl_seconds: u64,
    pub(crate) observation_retention_seconds: u64,
    pub(crate) pid_file: PathBuf,
    pub(crate) ui_dist_dir: PathBuf,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) bootstrap_enroll_tokens: Vec<String>,
    pub(crate) telemetry: WritableTelemetry,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct WritableTelemetry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) otlp_endpoint: Option<String>,
    pub(crate) service_name: String,
    pub(crate) json_logs: bool,
}

pub(crate) static DEFAULT_CONFIG: LazyLock<WritableConfig> = LazyLock::new(|| WritableConfig {
    data_dir: crate::cli::DEFAULT_DATA_DIR.into(),
    bind: "0.0.0.0:8080".to_string(),
    public_url: "http://127.0.0.1:8080".to_string(),
    state_file: "state.sqlite3".into(),
    command_timeout_ms: 30_000,
    enroll_token_ttl_seconds: 86_400,
    observation_retention_seconds: 2_592_000,
    pid_file: "wakey-control-plane.pid".into(),
    ui_dist_dir: "ui/dist".into(),
    bootstrap_enroll_tokens: Vec::new(),
    telemetry: WritableTelemetry {
        otlp_endpoint: None,
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
