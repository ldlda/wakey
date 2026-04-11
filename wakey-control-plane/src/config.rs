use std::net::SocketAddr;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::cli::ServeArgs;

#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub bind: SocketAddr,
    pub public_url: String,
    pub state_file: PathBuf,
    pub command_timeout: Duration,
    pub pid_file: PathBuf,
    pub enroll_tokens: Vec<String>,
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
struct FileConfig {
    bind: Option<String>,
    public_url: Option<String>,
    state_file: Option<PathBuf>,
    command_timeout_ms: Option<u64>,
    pid_file: Option<PathBuf>,
    enroll_tokens: Option<Vec<String>>,
    telemetry: Option<FileTelemetryConfig>,
}

#[derive(Debug, Deserialize, Default)]
struct FileTelemetryConfig {
    otlp_endpoint: Option<String>,
    service_name: Option<String>,
    json_logs: Option<bool>,
}

impl DaemonConfig {
    pub fn from_serve_args(args: &ServeArgs) -> Result<Self> {
        let file = load_file_config(&args.config_file)?;

        let bind = match args.bind {
            Some(bind) => bind,
            None => match file.bind {
                Some(ref bind) => bind
                    .parse::<SocketAddr>()
                    .with_context(|| format!("invalid bind address `{bind}` in {}", args.config_file.display()))?,
                None => "0.0.0.0:8080".parse().expect("static default bind should parse"),
            },
        };

        let public_url = normalize_public_url(
            args.public_url
                .as_deref()
                .or(file.public_url.as_deref())
                .unwrap_or("http://127.0.0.1:8080"),
        );

        let state_file = args
            .state_file
            .clone()
            .or(file.state_file)
            .unwrap_or_else(|| PathBuf::from(crate::cli::DEFAULT_STATE_FILE));

        let command_timeout = Duration::from_millis(
            args.command_timeout_ms
                .or(file.command_timeout_ms)
                .unwrap_or(30_000)
                .max(1),
        );

        let pid_file = args
            .pid_file
            .clone()
            .or(file.pid_file)
            .unwrap_or_else(|| PathBuf::from(crate::cli::DEFAULT_PID_FILE));

        let enroll_tokens = if args.enroll_tokens.is_empty() {
            file.enroll_tokens.unwrap_or_default()
        } else {
            args.enroll_tokens.clone()
        };

        let telemetry = resolve_telemetry(file.telemetry);

        Ok(Self {
            bind,
            public_url,
            state_file,
            command_timeout,
            pid_file,
            enroll_tokens,
            telemetry,
        })
    }
}

fn load_file_config(path: &Path) -> Result<FileConfig> {
    if !path.exists() {
        return Ok(FileConfig::default());
    }

    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    toml::from_str::<FileConfig>(&raw)
        .with_context(|| format!("failed to parse config file {}", path.display()))
}

fn resolve_telemetry(file: Option<FileTelemetryConfig>) -> TelemetryConfig {
    let mut out = TelemetryConfig::default();
    if let Some(file) = file {
        if let Some(endpoint) = file.otlp_endpoint {
            let trimmed = endpoint.trim().to_string();
            if !trimmed.is_empty() {
                out.otlp_endpoint = Some(trimmed);
            }
        }
        if let Some(name) = file.service_name {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                out.service_name = trimmed.to_string();
            }
        }
        if let Some(json_logs) = file.json_logs {
            out.json_logs = json_logs;
        }
    }
    out
}

pub fn normalize_public_url(url: &str) -> String {
    url.trim_end_matches('/').to_string()
}

pub fn issue_token_endpoint(base_url: &str) -> String {
    format!(
        "{}/api/v1/control/enroll-token",
        normalize_public_url(base_url)
    )
}
