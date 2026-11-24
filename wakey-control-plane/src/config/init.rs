use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cli::{InitConfigArgs, ServeArgs};
use crate::config::resolve::{load_file_config, resolve_path};
use crate::config::types::{DaemonConfig, FileConfig, FileTelemetryConfig};

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
    let source = args
        .from_config
        .as_deref()
        .unwrap_or_else(|| Path::new("<defaults>"));
    let config = DaemonConfig::from_file(base.overlay(args.into()), source)?;
    let body = renderable_config(config);

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

fn renderable_config(config: DaemonConfig) -> FileConfig {
    let state_file = path_for_rendered_config(&config.data_dir, config.state_file);
    let pid_file = path_for_rendered_config(&config.data_dir, config.pid_file);
    let telemetry = config.telemetry;
    FileConfig {
        access: config.access,
        data_dir: Some(config.data_dir),
        bind: Some(config.bind.to_string()),
        public_url: Some(config.public_url),
        state_file: Some(state_file),
        command_timeout: Some(config.command_timeout),
        enroll_token_ttl: Some(config.enroll_token_ttl),
        history_retention: Some(config.history_retention),
        device_retention: Some(config.device_retention),
        pid_file: Some(pid_file),
        ui_dist_dir: Some(config.ui_dist_dir),
        // The generated template omits empty bootstrap token lists.
        bootstrap_enroll_tokens: (!config.bootstrap_enroll_tokens.is_empty())
            .then_some(config.bootstrap_enroll_tokens),
        telemetry: Some(FileTelemetryConfig {
            console_filter: telemetry.console_filter,
            otlp_log_filter: telemetry.otlp_log_filter,
            trace_filter: telemetry.trace_filter,
            authorization_env: telemetry.authorization_env,
            otlp_endpoint: telemetry.otlp_endpoint,
            service_name: Some(telemetry.service_name),
            json_logs: Some(telemetry.json_logs),
        }),
    }
}

/// Keep relative paths relative when the rendered config's `data_dir` is also
/// relative. The daemon resolves relative paths under `data_dir`; embedding
/// that directory into another relative path here would make it get joined a
/// second time when the generated config is loaded.
fn path_for_rendered_config(data_dir: &std::path::Path, candidate: PathBuf) -> PathBuf {
    if data_dir.is_relative() && candidate.is_relative() {
        candidate
    } else {
        resolve_path(data_dir, candidate)
    }
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
        observation_retention_seconds: args.observation_retention_seconds,
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
    use crate::config::types::DEFAULT_CONFIG;

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
device_retention = "0d"
pid_file = "old.pid"
ui_dist_dir = "/opt/wakey/ui"
bootstrap_enroll_tokens = ["old-token"]

[access]
issuer = "https://access.example"
audience = "protected-app"
jwks_url = "https://keys.example/custom"

[telemetry]
otlp_endpoint = "http://otel.example"
authorization_env = "WAKEY_OTLP_AUTHORIZATION"
console_filter = "info"
otlp_log_filter = "debug"
trace_filter = "info"
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
            observation_retention_seconds: None,
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
        let converted: super::FileConfig = toml::from_str(&rendered).unwrap();
        assert_eq!(
            converted.command_timeout,
            Some(std::time::Duration::from_millis(999))
        );
        assert_eq!(
            converted.enroll_token_ttl,
            Some(std::time::Duration::from_secs(5678))
        );
        assert!(parsed.get("command_timeout_ms").is_none());
        assert_eq!(converted.device_retention, Some(std::time::Duration::ZERO));
        assert_eq!(
            converted.access,
            Some(crate::config::AccessConfig {
                issuer: "https://access.example".into(),
                audience: "protected-app".into(),
                jwks_url: Some("https://keys.example/custom".into()),
            })
        );
        assert!(parsed.get("enroll_token_ttl_seconds").is_none());
        assert_eq!(
            parsed["state_file"].as_str(),
            Some("/tmp/wakey-base/old.sqlite3")
        );
        assert_eq!(parsed["telemetry"]["json_logs"].as_bool(), Some(true));
        assert_eq!(parsed["telemetry"]["console_filter"].as_str(), Some("info"));
        assert_eq!(
            parsed["telemetry"]["otlp_log_filter"].as_str(),
            Some("debug")
        );
        assert_eq!(parsed["telemetry"]["trace_filter"].as_str(), Some("info"));
        assert_eq!(
            parsed["telemetry"]["authorization_env"].as_str(),
            Some("WAKEY_OTLP_AUTHORIZATION")
        );
        assert_eq!(
            parsed["telemetry"]["service_name"].as_str(),
            Some("old-service")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn init_config_renders_shared_default_template() {
        let dir = temp_test_dir("defaults");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let out = dir.join("out.toml");

        write_init_config(&InitConfigArgs {
            config_file: Some(out.clone()),
            from_config: None,
            stdout: false,
            data_dir: None,
            bind: None,
            public_url: None,
            state_file: None,
            pid_file: None,
            ui_dist_dir: None,
            command_timeout_ms: None,
            enroll_token_ttl_seconds: None,
            observation_retention_seconds: None,
            bootstrap_enroll_tokens: Vec::new(),
            telemetry_otlp_endpoint: None,
            telemetry_service_name: None,
            telemetry_json_logs: None,
            force: false,
        })
        .expect("write default config");

        let rendered = std::fs::read_to_string(&out).expect("read default config");
        let parsed: toml::Value = toml::from_str(&rendered).expect("parse default config");
        let defaults = &*DEFAULT_CONFIG;
        assert_eq!(
            parsed["state_file"].as_str(),
            defaults.data_dir.join(&defaults.state_file).to_str()
        );
        let converted: super::FileConfig = toml::from_str(&rendered).unwrap();
        assert_eq!(converted.command_timeout, Some(defaults.command_timeout));
        assert_eq!(converted.device_retention, Some(defaults.device_retention));
        assert_eq!(
            parsed["telemetry"]["service_name"].as_str(),
            Some(defaults.telemetry.service_name.as_str())
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn init_config_preserves_relative_paths_for_relative_data_dir() {
        let dir = temp_test_dir("relative-data-dir");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let base = dir.join("base.toml");
        let out = dir.join("out.toml");
        std::fs::write(
            &base,
            r#"
data_dir = "test-ddir/wakey-control-plane"
state_file = "state.sqlite3"
pid_file = "wakey-control-plane.pid"
"#,
        )
        .expect("write base config");

        write_init_config(&InitConfigArgs {
            config_file: Some(out.clone()),
            from_config: Some(base),
            stdout: false,
            data_dir: None,
            bind: None,
            public_url: None,
            state_file: None,
            pid_file: None,
            ui_dist_dir: None,
            command_timeout_ms: None,
            enroll_token_ttl_seconds: None,
            observation_retention_seconds: None,
            bootstrap_enroll_tokens: Vec::new(),
            telemetry_otlp_endpoint: None,
            telemetry_service_name: None,
            telemetry_json_logs: None,
            force: false,
        })
        .expect("write derived config");

        let parsed: toml::Value =
            toml::from_str(&std::fs::read_to_string(&out).expect("read derived config"))
                .expect("parse derived config");
        assert_eq!(parsed["state_file"].as_str(), Some("state.sqlite3"));
        assert_eq!(parsed["pid_file"].as_str(), Some("wakey-control-plane.pid"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn generated_config_uses_the_same_settings_as_serve() {
        use crate::cli::{Cli, Command};
        use crate::config::DaemonConfig;
        use clap::Parser;

        let dir = temp_test_dir("serve-round-trip");
        std::fs::create_dir_all(&dir).unwrap();
        let base = dir.join("base.toml");
        let out = dir.join("out.toml");
        std::fs::write(
            &base,
            "data_dir = 'old-data'\nstate_file = 'state.sqlite3'\npid_file = 'daemon.pid'\n\
             [telemetry]\nservice_name = '  cc-test  '\notlp_endpoint = '  '\njson_logs = true\n",
        )
        .unwrap();
        let Command::InitConfig(init) = Cli::parse_from([
            "wakey-control-plane",
            "init-config",
            "--config-file",
            out.to_str().unwrap(),
            "--from-config",
            base.to_str().unwrap(),
            "--data-dir",
            "new-data",
            "--telemetry-json-logs=false",
        ])
        .command
        else {
            panic!("expected init-config");
        };
        write_init_config(&init).unwrap();

        let Command::Serve(serve) = Cli::parse_from([
            "wakey-control-plane",
            "serve",
            "--config-file",
            out.to_str().unwrap(),
        ])
        .command
        else {
            panic!("expected serve");
        };
        let resolved = DaemonConfig::from_serve_args(&serve).unwrap();
        assert_eq!(
            resolved.state_file,
            std::path::Path::new("new-data/state.sqlite3")
        );
        assert_eq!(
            resolved.pid_file,
            std::path::Path::new("new-data/daemon.pid")
        );
        assert_eq!(resolved.telemetry.service_name, "cc-test");
        assert!(!resolved.telemetry.json_logs);
        let rendered = super::load_file_config(&out).unwrap().telemetry.unwrap();
        assert_eq!(
            rendered.service_name.as_deref(),
            Some(resolved.telemetry.service_name.as_str())
        );
        assert_eq!(rendered.otlp_endpoint, resolved.telemetry.otlp_endpoint);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
