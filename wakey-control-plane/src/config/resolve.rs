use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::cli::{
    AdminTargetArgs, IssueEnrollTokenArgs, ListEnrollTokensArgs, RevokeAgentArgs,
    RevokeEnrollTokenArgs, ServeArgs, StateStatsArgs,
};
use crate::config::types::{
    DEFAULT_CONFIG, DaemonConfig, FileConfig, FileTelemetryConfig, IssueTokenSettings,
    StateAccessSettings, TelemetryConfig,
};

impl DaemonConfig {
    pub fn from_serve_args(args: &ServeArgs) -> Result<Self> {
        let file = load_file_config(&args.config_file)?;
        let defaults = &*DEFAULT_CONFIG;

        let data_dir = args
            .data_dir
            .clone()
            .or(file.data_dir.clone())
            .unwrap_or_else(|| defaults.data_dir.clone());

        let bind = match args.bind {
            Some(bind) => bind,
            None => match file.bind {
                Some(ref bind) => bind.parse::<SocketAddr>().with_context(|| {
                    format!(
                        "invalid bind address `{bind}` in {}",
                        args.config_file.display()
                    )
                })?,
                None => defaults.bind,
            },
        };

        let public_url = normalize_public_url(
            args.public_url
                .as_deref()
                .or(file.public_url.as_deref())
                .unwrap_or(&defaults.public_url),
        );

        let state_file_raw = args
            .state_file
            .clone()
            .or(file.state_file)
            .unwrap_or_else(|| defaults.state_file.clone());
        let state_file = resolve_path(&data_dir, state_file_raw);

        let command_timeout = args
            .command_timeout_ms
            .map(Duration::from_millis)
            .or(file.command_timeout)
            .unwrap_or(defaults.command_timeout);
        let enroll_token_ttl = args
            .enroll_token_ttl_seconds
            .map(Duration::from_secs)
            .or(file.enroll_token_ttl)
            .unwrap_or(defaults.enroll_token_ttl);
        let history_retention = args
            .observation_retention_seconds
            .map(Duration::from_secs)
            .or(file.history_retention)
            .unwrap_or(defaults.history_retention);
        validate_durations(command_timeout, enroll_token_ttl, history_retention)?;

        let pid_file_raw = args
            .pid_file
            .clone()
            .or(file.pid_file)
            .unwrap_or_else(|| defaults.pid_file.clone());
        let pid_file = resolve_path(&data_dir, pid_file_raw);

        let ui_dist_dir = args
            .ui_dist_dir
            .clone()
            .or(file.ui_dist_dir)
            .unwrap_or_else(|| defaults.ui_dist_dir.clone());

        let bootstrap_enroll_tokens = if args.bootstrap_enroll_tokens.is_empty() {
            file.bootstrap_enroll_tokens
                .unwrap_or_else(|| defaults.bootstrap_enroll_tokens.clone())
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
            history_retention,
            pid_file,
            ui_dist_dir,
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
    let defaults = &*DEFAULT_CONFIG;
    let state = resolve_state_access(
        &args.config_file,
        args.data_dir.clone(),
        args.state_file.clone(),
        args.public_url.clone(),
        &args.target,
    )?;

    let ttl = args
        .ttl_seconds
        .map(Duration::from_secs)
        .or(file.enroll_token_ttl)
        .unwrap_or(defaults.enroll_token_ttl);
    validate_durations(defaults.command_timeout, ttl, defaults.history_retention)?;

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

pub fn resolve_revoke_agent_settings(args: &RevokeAgentArgs) -> Result<StateAccessSettings> {
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
    let raw: toml::Value = toml::from_str(&raw)
        .with_context(|| format!("failed to parse config file {}", path.display()))?;
    let mut value = serde_json::to_value(raw)?;
    let deprecated =
        wakey_core::config_duration::normalize(&mut value, super::types::DURATION_KEYS)
            .map_err(anyhow::Error::msg)?;
    let config = serde_json::from_value(value)
        .with_context(|| format!("failed to parse config file {}", path.display()))?;
    for key in deprecated {
        // Daemon config is loaded before telemetry initialization. Keep these
        // warnings on stderr, including when init-config emits TOML on stdout.
        eprintln!(
            "warning: {}: `{}` is deprecated; use `{}` with a duration string",
            path.display(),
            key.old,
            key.new
        );
    }
    Ok(config)
}

pub(crate) fn validate_durations(
    command_timeout: Duration,
    enroll_token_ttl: Duration,
    history_retention: Duration,
) -> Result<()> {
    for (name, value) in [
        ("command_timeout", command_timeout),
        ("enroll_token_ttl", enroll_token_ttl),
        ("history_retention", history_retention),
    ] {
        anyhow::ensure!(!value.is_zero(), "{name} must be positive");
        anyhow::ensure!(
            std::time::Instant::now().checked_add(value).is_some(),
            "{name} exceeds the platform timer range"
        );
    }
    anyhow::ensure!(
        enroll_token_ttl.subsec_nanos() == 0,
        "enroll_token_ttl must use whole seconds"
    );
    anyhow::ensure!(
        history_retention.subsec_nanos() == 0,
        "history_retention must use whole seconds"
    );
    Ok(())
}

fn resolve_admin_public_url(
    mode: &AdminTargetArgs,
    config_public_url: Option<String>,
) -> Result<Option<String>> {
    match (mode.live, mode.offline) {
        (true, false) => config_public_url
            .ok_or_else(|| {
                anyhow::anyhow!("--live requires --public-url or a configured public_url")
            })
            .map(Some),
        (false, true) => Ok(None),
        _ => Ok(config_public_url),
    }
}

fn resolve_telemetry(file: Option<FileTelemetryConfig>) -> TelemetryConfig {
    // Service-specific defaults live in the CC template, not the shared type.
    let mut out = DEFAULT_CONFIG.telemetry.clone();
    if let Some(file) = file {
        out.authorization_env = file.authorization_env;
        out.console_filter = file.console_filter;
        out.otlp_log_filter = file.otlp_log_filter;
        out.trace_filter = file.trace_filter;
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
    let defaults = &*DEFAULT_CONFIG;

    let data_dir = cli_data_dir
        .or(file.data_dir)
        .unwrap_or_else(|| defaults.data_dir.clone());

    let state_file = resolve_path(
        &data_dir,
        cli_state_file
            .or(file.state_file)
            .unwrap_or_else(|| defaults.state_file.clone()),
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
    use super::{DaemonConfig, normalize_public_url, resolve_path};
    use crate::cli::ServeArgs;
    use crate::config::types::DEFAULT_CONFIG;
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
            PathBuf::from("state.sqlite3"),
        );
        assert_eq!(
            out,
            PathBuf::from("/var/lib/wakey-control-plane/state.sqlite3")
        );
    }

    #[test]
    fn daemon_config_uses_shared_default_template() {
        let config_file = std::env::temp_dir().join(format!(
            "wakey-cc-missing-config-{}.toml",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&config_file);
        let resolved = DaemonConfig::from_serve_args(&ServeArgs {
            data_dir: None,
            bind: None,
            public_url: None,
            state_file: None,
            bootstrap_enroll_tokens: Vec::new(),
            command_timeout_ms: None,
            enroll_token_ttl_seconds: None,
            observation_retention_seconds: None,
            pid_file: None,
            ui_dist_dir: None,
            config_file,
            bootstrap_config: false,
        })
        .expect("resolve default config");
        let defaults = &*DEFAULT_CONFIG;

        assert_eq!(resolved.data_dir, defaults.data_dir);
        assert_eq!(
            resolved.state_file,
            defaults.data_dir.join(&defaults.state_file)
        );
        assert_eq!(resolved.command_timeout, defaults.command_timeout);
        assert_eq!(
            resolved.telemetry.service_name,
            defaults.telemetry.service_name
        );
    }

    #[test]
    fn invalid_duration_policies_are_rejected_without_clamping() {
        use std::time::Duration;
        let defaults = &*DEFAULT_CONFIG;
        assert!(
            super::validate_durations(
                Duration::ZERO,
                defaults.enroll_token_ttl,
                defaults.history_retention
            )
            .is_err()
        );
        assert!(
            super::validate_durations(
                defaults.command_timeout,
                Duration::from_millis(500),
                defaults.history_retention
            )
            .is_err()
        );
        assert!(
            super::validate_durations(
                defaults.command_timeout,
                defaults.enroll_token_ttl,
                Duration::ZERO
            )
            .is_err()
        );
        assert!(
            super::validate_durations(
                Duration::from_secs(u64::MAX),
                defaults.enroll_token_ttl,
                defaults.history_retention
            )
            .is_err()
        );
    }
}
