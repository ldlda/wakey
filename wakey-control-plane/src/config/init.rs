use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::cli::{InitConfigArgs, ServeArgs};
use crate::config::resolve::{normalize_public_url, resolve_path};
use crate::config::types::{WritableConfig, WritableTelemetry};

pub fn write_init_config(args: &InitConfigArgs) -> Result<()> {
    if args.config_file.exists() && !args.force {
        anyhow::bail!(
            "config {} already exists; re-run with --force to overwrite",
            args.config_file.display()
        );
    }

    let bind = args.bind.unwrap_or_else(|| {
        "0.0.0.0:8080"
            .parse()
            .expect("static default bind should parse")
    });
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
        bootstrap_enroll_tokens: args.bootstrap_enroll_tokens.clone(),
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
        bootstrap_enroll_tokens: args.bootstrap_enroll_tokens.clone(),
        telemetry_otlp_endpoint: None,
        telemetry_service_name: None,
        telemetry_json_logs: false,
        force: false,
    };

    write_init_config(&init)?;
    Ok(true)
}
