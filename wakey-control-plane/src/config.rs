use std::net::SocketAddr;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::cli::{
    InitConfigArgs, IssueEnrollTokenArgs, ListEnrollTokensArgs, RevokeEnrollTokenArgs,
    ServeArgs, StateStatsArgs,
};

#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub data_dir: PathBuf,
    pub bind: SocketAddr,
    pub public_url: String,
    pub state_file: PathBuf,
    pub command_timeout: Duration,
    pub enroll_token_ttl: Duration,
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
    data_dir: Option<PathBuf>,
    bind: Option<String>,
    public_url: Option<String>,
    state_file: Option<PathBuf>,
    command_timeout_ms: Option<u64>,
    enroll_token_ttl_seconds: Option<u64>,
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

#[derive(Debug, Serialize)]
struct WritableConfig {
    data_dir: PathBuf,
    bind: String,
    public_url: String,
    state_file: PathBuf,
    command_timeout_ms: u64,
    enroll_token_ttl_seconds: u64,
    pid_file: PathBuf,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    enroll_tokens: Vec<String>,
    telemetry: WritableTelemetry,
}

#[derive(Debug, Serialize)]
struct WritableTelemetry {
    #[serde(skip_serializing_if = "Option::is_none")]
    otlp_endpoint: Option<String>,
    service_name: String,
    json_logs: bool,
}

impl DaemonConfig {
    pub fn from_serve_args(args: &ServeArgs) -> Result<Self> {
        let file = load_file_config(&args.config_file)?;

        let data_dir = args
            .data_dir
            .clone()
            .or(file.data_dir.clone())
            .unwrap_or_else(|| PathBuf::from(crate::cli::DEFAULT_DATA_DIR));

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

        let state_file_raw = args
            .state_file
            .clone()
            .or(file.state_file)
            .unwrap_or_else(|| PathBuf::from("state.db"));
        let state_file = resolve_path(&data_dir, state_file_raw);

        let command_timeout = Duration::from_millis(
            args.command_timeout_ms
                .or(file.command_timeout_ms)
                .unwrap_or(30_000)
                .max(1),
        );

        let enroll_token_ttl = Duration::from_secs(
            args.enroll_token_ttl_seconds
                .or(file.enroll_token_ttl_seconds)
                .unwrap_or(86_400)
                .max(1),
        );

        let pid_file_raw = args
            .pid_file
            .clone()
            .or(file.pid_file)
            .unwrap_or_else(|| PathBuf::from("wakey-control-plane.pid"));
        let pid_file = resolve_path(&data_dir, pid_file_raw);

        let enroll_tokens = if args.enroll_tokens.is_empty() {
            file.enroll_tokens.unwrap_or_default()
        } else {
            args.enroll_tokens.clone()
        };

        let telemetry = resolve_telemetry(file.telemetry);

        Ok(Self {
            data_dir,
            bind,
            public_url,
            state_file,
            command_timeout,
            enroll_token_ttl,
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

pub fn write_init_config(args: &InitConfigArgs) -> Result<()> {
    if args.config_file.exists() && !args.force {
        anyhow::bail!(
            "config {} already exists; re-run with --force to overwrite",
            args.config_file.display()
        );
    }

    let bind = args
        .bind
        .unwrap_or_else(|| "0.0.0.0:8080".parse().expect("static default bind should parse"));
    let public_url = normalize_public_url(
        args.public_url
            .as_deref()
            .unwrap_or("http://127.0.0.1:8080"),
    );

    let data_dir = args
        .data_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from(crate::cli::DEFAULT_DATA_DIR));

    let state_file_raw = args
        .state_file
        .clone()
        .unwrap_or_else(|| PathBuf::from("state.db"));
    let state_file = resolve_path(&data_dir, state_file_raw);

    let pid_file_raw = args
        .pid_file
        .clone()
        .unwrap_or_else(|| PathBuf::from("wakey-control-plane.pid"));
    let pid_file = resolve_path(&data_dir, pid_file_raw);

    let body = WritableConfig {
        data_dir,
        bind: bind.to_string(),
        public_url,
        state_file,
        command_timeout_ms: args.command_timeout_ms.unwrap_or(30_000).max(1),
        enroll_token_ttl_seconds: args.enroll_token_ttl_seconds.unwrap_or(86_400).max(1),
        pid_file,
        enroll_tokens: args.enroll_tokens.clone(),
        telemetry: WritableTelemetry {
            otlp_endpoint: args.telemetry_otlp_endpoint.clone(),
            service_name: args
                .telemetry_service_name
                .clone()
                .unwrap_or_else(|| "wakey-control-plane".to_string()),
            json_logs: args.telemetry_json_logs,
        },
    };

    if let Some(parent) = args.config_file.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config dir {}", parent.display()))?;
    }

    let rendered = toml::to_string_pretty(&body).context("failed to render config template")?;
    std::fs::write(&args.config_file, rendered)
        .with_context(|| format!("failed to write config {}", args.config_file.display()))?;
    Ok(())
}

pub fn bootstrap_config_if_missing(args: &ServeArgs) -> Result<bool> {
    if args.config_file.exists() {
        return Ok(false);
    }

    let init = InitConfigArgs {
        config_file: args.config_file.clone(),
        data_dir: args.data_dir.clone(),
        bind: args.bind,
        public_url: args.public_url.clone(),
        state_file: args.state_file.clone(),
        pid_file: args.pid_file.clone(),
        command_timeout_ms: args.command_timeout_ms,
        enroll_token_ttl_seconds: args.enroll_token_ttl_seconds,
        enroll_tokens: args.enroll_tokens.clone(),
        telemetry_otlp_endpoint: None,
        telemetry_service_name: None,
        telemetry_json_logs: false,
        force: false,
    };

    write_init_config(&init)?;
    Ok(true)
}

pub fn resolve_path(data_dir: &Path, candidate: PathBuf) -> PathBuf {
    if candidate.is_absolute() {
        candidate
    } else {
        data_dir.join(candidate)
    }
}

pub struct IssueTokenSettings {
    pub data_dir: PathBuf,
    pub state_file: PathBuf,
    pub ttl: Duration,
}

pub struct StateAccessSettings {
    pub data_dir: PathBuf,
    pub state_file: PathBuf,
    pub public_url: Option<String>,
}

pub fn resolve_issue_token_settings(args: &IssueEnrollTokenArgs) -> Result<IssueTokenSettings> {
    let file = load_file_config(&args.config_file)?;
    let state = resolve_state_access(
        &args.config_file,
        args.data_dir.clone(),
        args.state_file.clone(),
        args.public_url.clone(),
    )?;

    let ttl = Duration::from_secs(
        args.ttl_seconds
            .or(file.enroll_token_ttl_seconds)
            .unwrap_or(86_400)
            .max(1),
    );

    Ok(IssueTokenSettings {
        data_dir: state.data_dir,
        state_file: state.state_file,
        ttl,
    })
}

pub fn resolve_list_enroll_token_settings(args: &ListEnrollTokensArgs) -> Result<StateAccessSettings> {
    resolve_state_access(
        &args.config_file,
        args.data_dir.clone(),
        args.state_file.clone(),
        args.public_url.clone(),
    )
}

pub fn resolve_revoke_enroll_token_settings(args: &RevokeEnrollTokenArgs) -> Result<StateAccessSettings> {
    resolve_state_access(
        &args.config_file,
        args.data_dir.clone(),
        args.state_file.clone(),
        args.public_url.clone(),
    )
}

pub fn resolve_state_stats_settings(args: &StateStatsArgs) -> Result<StateAccessSettings> {
    resolve_state_access(
        &args.config_file,
        args.data_dir.clone(),
        args.state_file.clone(),
        args.public_url.clone(),
    )
}

fn resolve_state_access(
    config_file: &Path,
    cli_data_dir: Option<PathBuf>,
    cli_state_file: Option<PathBuf>,
    cli_public_url: Option<String>,
) -> Result<StateAccessSettings> {
    let file = load_file_config(config_file)?;

    let data_dir = cli_data_dir
        .or(file.data_dir)
        .unwrap_or_else(|| PathBuf::from(crate::cli::DEFAULT_DATA_DIR));

    let state_file = resolve_path(
        &data_dir,
        cli_state_file
            .or(file.state_file)
            .unwrap_or_else(|| PathBuf::from("state.db")),
    );

    let public_url = cli_public_url
        .or(file.public_url)
        .map(|url| normalize_public_url(&url));

    Ok(StateAccessSettings {
        data_dir,
        state_file,
        public_url,
    })
}
