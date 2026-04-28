use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::cli::{InitConfigArgs, ServeArgs};
use crate::config::resolve::{load_file_config, normalize_public_url, resolve_path};
use crate::config::types::{WritableConfig, WritableTelemetry};

pub fn write_init_config(args: &InitConfigArgs) -> Result<Option<PathBuf>> {
    if args.stdout && args.config_file.is_some() {
        anyhow::bail!("--stdout cannot be used with --config-file");
    }

    if let Some(config_file) = &args.config_file
        && config_file.exists()
        && !args.force
    {
        anyhow::bail!(
            "config {} already exists; re-run with --force to overwrite",
            config_file.display()
        );
    }

    let base = if let Some(from_config) = &args.from_config {
        if !from_config.exists() {
            anyhow::bail!("base config {} does not exist", from_config.display());
        }
        load_file_config(from_config)?
    } else {
        Default::default()
    };
    let base_telemetry = base.telemetry.unwrap_or_default();

    let bind = match args.bind {
        Some(bind) => bind,
        None => match base.bind {
            Some(ref bind) => bind.parse().with_context(|| {
                format!(
                    "invalid bind address `{bind}` in base config {}",
                    args.from_config
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "<defaults>".to_string())
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
            .or(base.public_url.as_deref())
            .unwrap_or("http://127.0.0.1:8080"),
    );

    let data_dir = args
        .data_dir
        .clone()
        .or(base.data_dir)
        .unwrap_or_else(|| PathBuf::from(crate::cli::DEFAULT_DATA_DIR));

    let state_file_raw = args
        .state_file
        .clone()
        .or(base.state_file)
        .unwrap_or_else(|| PathBuf::from("state.sqlite3"));
    let state_file = resolve_path(&data_dir, state_file_raw);

    let pid_file_raw = args
        .pid_file
        .clone()
        .or(base.pid_file)
        .unwrap_or_else(|| PathBuf::from("wakey-control-plane.pid"));
    let pid_file = resolve_path(&data_dir, pid_file_raw);

    let ui_dist_dir = args
        .ui_dist_dir
        .clone()
        .or(base.ui_dist_dir)
        .unwrap_or_else(|| PathBuf::from("ui/dist"));

    let bootstrap_enroll_tokens = if args.bootstrap_enroll_tokens.is_empty() {
        base.bootstrap_enroll_tokens.unwrap_or_default()
    } else {
        args.bootstrap_enroll_tokens.clone()
    };

    let body = WritableConfig {
        data_dir,
        bind: bind.to_string(),
        public_url,
        state_file,
        command_timeout_ms: args
            .command_timeout_ms
            .or(base.command_timeout_ms)
            .unwrap_or(30_000)
            .max(1),
        enroll_token_ttl_seconds: args
            .enroll_token_ttl_seconds
            .or(base.enroll_token_ttl_seconds)
            .unwrap_or(86_400)
            .max(1),
        pid_file,
        ui_dist_dir,
        bootstrap_enroll_tokens,
        telemetry: WritableTelemetry {
            otlp_endpoint: args
                .telemetry_otlp_endpoint
                .clone()
                .or(base_telemetry.otlp_endpoint),
            service_name: args
                .telemetry_service_name
                .clone()
                .or(base_telemetry.service_name)
                .unwrap_or_else(|| "wakey-control-plane".to_string()),
            json_logs: args
                .telemetry_json_logs
                .or(base_telemetry.json_logs)
                .unwrap_or(false),
        },
    };

    let rendered = toml::to_string_pretty(&body).context("failed to render config template")?;

    if let Some(config_file) = &args.config_file {
        if let Some(parent) = config_file.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create config dir {}", parent.display()))?;
        }

        std::fs::write(config_file, rendered)
            .with_context(|| format!("failed to write config {}", config_file.display()))?;
        return Ok(Some(config_file.clone()));
    }

    print!("{}", rendered);
    Ok(None)
}

pub fn bootstrap_config_if_missing(args: &ServeArgs) -> Result<bool> {
    if args.config_file.exists() {
        return Ok(false);
    }

    let init = InitConfigArgs {
        config_file: Some(args.config_file.clone()),
        stdout: false,
        data_dir: args.data_dir.clone(),
        bind: args.bind,
        public_url: args.public_url.clone(),
        state_file: args.state_file.clone(),
        pid_file: args.pid_file.clone(),
        ui_dist_dir: args.ui_dist_dir.clone(),
        command_timeout_ms: args.command_timeout_ms,
        enroll_token_ttl_seconds: args.enroll_token_ttl_seconds,
        bootstrap_enroll_tokens: args.bootstrap_enroll_tokens.clone(),
        from_config: None,
        telemetry_otlp_endpoint: None,
        telemetry_service_name: None,
        telemetry_json_logs: None,
        force: false,
    };

    write_init_config(&init)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::write_init_config;
    use crate::cli::InitConfigArgs;

    fn temp_test_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "wakey-cc-init-config-{name}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn init_config_from_config_preserves_base_and_applies_overrides() {
        let dir = temp_test_dir("from-config");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let base = dir.join("base.toml");
        let out = dir.join("out.toml");
        std::fs::write(
            &base,
            r#"
data_dir = "/tmp/wakey-base"
bind = "127.0.0.1:9090"
public_url = "https://old.example/"
state_file = "old.sqlite3"
command_timeout_ms = 1234
enroll_token_ttl_seconds = 5678
pid_file = "old.pid"
ui_dist_dir = "/opt/wakey/ui"
bootstrap_enroll_tokens = ["old-token"]

[telemetry]
otlp_endpoint = "http://otel.example"
service_name = "old-service"
json_logs = true
"#,
        )
        .expect("write base config");

        write_init_config(&InitConfigArgs {
            config_file: Some(out.clone()),
            from_config: Some(base),
            stdout: false,
            data_dir: None,
            bind: None,
            public_url: Some("https://new.example/".to_string()),
            state_file: None,
            pid_file: None,
            ui_dist_dir: None,
            command_timeout_ms: Some(999),
            enroll_token_ttl_seconds: None,
            bootstrap_enroll_tokens: Vec::new(),
            telemetry_otlp_endpoint: None,
            telemetry_service_name: None,
            telemetry_json_logs: None,
            force: false,
        })
        .expect("write derived config");

        let rendered = std::fs::read_to_string(&out).expect("read derived config");
        let parsed: toml::Value = toml::from_str(&rendered).expect("parse derived config");
        assert_eq!(parsed["public_url"].as_str(), Some("https://new.example"));
        assert_eq!(parsed["bind"].as_str(), Some("127.0.0.1:9090"));
        assert_eq!(parsed["command_timeout_ms"].as_integer(), Some(999));
        assert_eq!(parsed["enroll_token_ttl_seconds"].as_integer(), Some(5678));
        assert_eq!(
            parsed["state_file"].as_str(),
            Some("/tmp/wakey-base/old.sqlite3")
        );
        assert_eq!(parsed["telemetry"]["json_logs"].as_bool(), Some(true));
        assert_eq!(
            parsed["telemetry"]["service_name"].as_str(),
            Some("old-service")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
