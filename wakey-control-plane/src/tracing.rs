use crate::config::TelemetryConfig;
use anyhow::Result;
pub use wakey_core::telemetry::TelemetryGuard;

pub fn init(verbose: u8, telemetry: &TelemetryConfig) -> Result<TelemetryGuard> {
    let config = wakey_core::telemetry::TelemetryConfig {
        otlp_endpoint: telemetry.otlp_endpoint.clone(),
        authorization_env: telemetry.authorization_env.clone(),
        service_name: telemetry.service_name.clone(),
        json_logs: telemetry.json_logs,
    };
    wakey_core::telemetry::init(
        &config,
        "wakey-control-plane",
        match verbose {
            0 => "wakey_control_plane=info,wakey_core=info,opentelemetry=warn",
            1 => "wakey_control_plane=debug,wakey_core=info,opentelemetry=warn",
            _ => "wakey_control_plane=trace,wakey_core=info,opentelemetry=warn",
        },
        None,
    )
}
