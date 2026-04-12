use std::net::SocketAddr;
use std::path::PathBuf;
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
    pub pid_file: PathBuf,
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
        Self {
            otlp_endpoint: None,
            service_name: "wakey-control-plane".to_string(),
            json_logs: false,
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
    pub(crate) pid_file: Option<PathBuf>,
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

#[derive(Debug, Serialize)]
pub(crate) struct WritableConfig {
    pub(crate) data_dir: PathBuf,
    pub(crate) bind: String,
    pub(crate) public_url: String,
    pub(crate) state_file: PathBuf,
    pub(crate) command_timeout_ms: u64,
    pub(crate) enroll_token_ttl_seconds: u64,
    pub(crate) pid_file: PathBuf,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) bootstrap_enroll_tokens: Vec<String>,
    pub(crate) telemetry: WritableTelemetry,
}

#[derive(Debug, Serialize)]
pub(crate) struct WritableTelemetry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) otlp_endpoint: Option<String>,
    pub(crate) service_name: String,
    pub(crate) json_logs: bool,
}

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
