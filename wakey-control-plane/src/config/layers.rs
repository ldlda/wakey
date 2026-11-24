use std::time::Duration;

use crate::cli::{InitConfigArgs, ServeArgs};

use super::types::{FileConfig, FileTelemetryConfig};

impl FileConfig {
    /// Apply only supplied values. An explicit false, zero, or empty list wins.
    pub(super) fn overlay(self, overrides: Self) -> Self {
        let telemetry = match (self.telemetry, overrides.telemetry) {
            (Some(base), Some(overrides)) => Some(base.overlay(overrides)),
            (base, overrides) => overrides.or(base),
        };
        Self {
            data_dir: overrides.data_dir.or(self.data_dir),
            bind: overrides.bind.or(self.bind),
            public_url: overrides.public_url.or(self.public_url),
            state_file: overrides.state_file.or(self.state_file),
            command_timeout: overrides.command_timeout.or(self.command_timeout),
            enroll_token_ttl: overrides.enroll_token_ttl.or(self.enroll_token_ttl),
            history_retention: overrides.history_retention.or(self.history_retention),
            device_retention: overrides.device_retention.or(self.device_retention),
            pid_file: overrides.pid_file.or(self.pid_file),
            ui_dist_dir: overrides.ui_dist_dir.or(self.ui_dist_dir),
            bootstrap_enroll_tokens: overrides
                .bootstrap_enroll_tokens
                .or(self.bootstrap_enroll_tokens),
            access: overrides.access.or(self.access),
            telemetry,
        }
    }
}

impl FileTelemetryConfig {
    fn overlay(self, overrides: Self) -> Self {
        Self {
            console_filter: overrides.console_filter.or(self.console_filter),
            otlp_log_filter: overrides.otlp_log_filter.or(self.otlp_log_filter),
            trace_filter: overrides.trace_filter.or(self.trace_filter),
            authorization_env: overrides.authorization_env.or(self.authorization_env),
            otlp_endpoint: overrides.otlp_endpoint.or(self.otlp_endpoint),
            service_name: overrides.service_name.or(self.service_name),
            json_logs: overrides.json_logs.or(self.json_logs),
        }
    }
}

impl From<&ServeArgs> for FileConfig {
    fn from(args: &ServeArgs) -> Self {
        Self {
            data_dir: args.data_dir.clone(),
            bind: args.bind.map(|bind| bind.to_string()),
            public_url: args.public_url.clone(),
            state_file: args.state_file.clone(),
            pid_file: args.pid_file.clone(),
            ui_dist_dir: args.ui_dist_dir.clone(),
            command_timeout: args.command_timeout_ms.map(Duration::from_millis),
            enroll_token_ttl: args.enroll_token_ttl_seconds.map(Duration::from_secs),
            history_retention: args.observation_retention_seconds.map(Duration::from_secs),
            bootstrap_enroll_tokens: (!args.bootstrap_enroll_tokens.is_empty())
                .then(|| args.bootstrap_enroll_tokens.clone()),
            ..Self::default()
        }
    }
}

impl From<&InitConfigArgs> for FileConfig {
    fn from(args: &InitConfigArgs) -> Self {
        Self {
            data_dir: args.data_dir.clone(),
            bind: args.bind.map(|bind| bind.to_string()),
            public_url: args.public_url.clone(),
            state_file: args.state_file.clone(),
            pid_file: args.pid_file.clone(),
            ui_dist_dir: args.ui_dist_dir.clone(),
            command_timeout: args.command_timeout_ms.map(Duration::from_millis),
            enroll_token_ttl: args.enroll_token_ttl_seconds.map(Duration::from_secs),
            history_retention: args.observation_retention_seconds.map(Duration::from_secs),
            bootstrap_enroll_tokens: (!args.bootstrap_enroll_tokens.is_empty())
                .then(|| args.bootstrap_enroll_tokens.clone()),
            telemetry: Some(FileTelemetryConfig {
                otlp_endpoint: args.telemetry_otlp_endpoint.clone(),
                service_name: args.telemetry_service_name.clone(),
                json_logs: args.telemetry_json_logs,
                ..FileTelemetryConfig::default()
            }),
            ..Self::default()
        }
    }
}
