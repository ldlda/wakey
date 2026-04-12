use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::cli::{
    AdminTargetArgs, IssueEnrollTokenArgs, ListEnrollTokensArgs, RevokeEnrollTokenArgs, ServeArgs,
    StateStatsArgs,
};
use crate::config::types::{
    DaemonConfig, FileConfig, FileTelemetryConfig, IssueTokenSettings, StateAccessSettings,
    TelemetryConfig,
};

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
                Some(ref bind) => bind.parse::<SocketAddr>().with_context(|| {
                    format!(
                        "invalid bind address `{bind}` in {}",
                        args.config_file.display()
                    )
                })?,
                None => "0.0.0.0:8080"
                    .parse()
                    .expect("static default bind should parse"),
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

        let bootstrap_enroll_tokens = if args.bootstrap_enroll_tokens.is_empty() {
            file.bootstrap_enroll_tokens.unwrap_or_default()
        } else {
            args.bootstrap_enroll_tokens.clone()
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
            bootstrap_enroll_tokens,
            telemetry,
        })
    }
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

pub fn resolve_path(data_dir: &Path, candidate: PathBuf) -> PathBuf {
    if candidate.is_absolute() {
        candidate
    } else {
        data_dir.join(candidate)
    }
}

pub fn resolve_issue_token_settings(args: &IssueEnrollTokenArgs) -> Result<IssueTokenSettings> {
    let file = load_file_config(&args.config_file)?;
    let state = resolve_state_access(
        &args.config_file,
        args.data_dir.clone(),
        args.state_file.clone(),
        args.public_url.clone(),
        &args.target,
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
        public_url: state.public_url,
        ttl,
    })
}

pub fn resolve_list_enroll_token_settings(
    args: &ListEnrollTokensArgs,
) -> Result<StateAccessSettings> {
    resolve_state_access(
        &args.config_file,
        args.data_dir.clone(),
        args.state_file.clone(),
        args.public_url.clone(),
        &args.target,
    )
}

pub fn resolve_revoke_enroll_token_settings(
    args: &RevokeEnrollTokenArgs,
) -> Result<StateAccessSettings> {
    resolve_state_access(
        &args.config_file,
        args.data_dir.clone(),
        args.state_file.clone(),
        args.public_url.clone(),
        &args.target,
    )
}

pub fn resolve_state_stats_settings(args: &StateStatsArgs) -> Result<StateAccessSettings> {
    resolve_state_access(
        &args.config_file,
        args.data_dir.clone(),
        args.state_file.clone(),
        args.public_url.clone(),
        &args.target,
    )
}

pub(crate) fn load_file_config(path: &Path) -> Result<FileConfig> {
    if !path.exists() {
        return Ok(FileConfig::default());
    }

    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    toml::from_str::<FileConfig>(&raw)
        .with_context(|| format!("failed to parse config file {}", path.display()))
}

fn resolve_admin_public_url(
    mode: &AdminTargetArgs,
    config_public_url: Option<String>,
) -> Result<Option<String>> {
    match (mode.live, mode.offline) {
        (true, false) => config_public_url
            .ok_or_else(|| anyhow::anyhow!("--live requires --public-url or a configured public_url"))
            .map(Some),
        (false, true) => Ok(None),
        _ => Ok(config_public_url),
    }
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

fn resolve_state_access(
    config_file: &Path,
    cli_data_dir: Option<PathBuf>,
    cli_state_file: Option<PathBuf>,
    cli_public_url: Option<String>,
    mode: &AdminTargetArgs,
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

    let resolved_public_url = cli_public_url
        .or(file.public_url)
        .map(|url| normalize_public_url(&url));
    let public_url = resolve_admin_public_url(mode, resolved_public_url)?;

    Ok(StateAccessSettings {
        data_dir,
        state_file,
        public_url,
    })
}

#[cfg(test)]
mod tests {
    use super::{normalize_public_url, resolve_path};
    use std::path::Path;
    use std::path::PathBuf;

    #[test]
    fn normalize_public_url_trims_trailing_slash() {
        assert_eq!(
            normalize_public_url("https://cp.example.com/"),
            "https://cp.example.com"
        );
    }

    #[test]
    fn resolve_path_joins_relative_path() {
        let out = resolve_path(
            Path::new("/var/lib/wakey-control-plane"),
            PathBuf::from("state.db"),
        );
        assert_eq!(out, PathBuf::from("/var/lib/wakey-control-plane/state.db"));
    }
}
