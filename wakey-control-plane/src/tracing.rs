use crate::config::TelemetryConfig;
use anyhow::Result;
pub use wakey_core::telemetry::TelemetryGuard;

pub fn init(verbose: u8, telemetry: &TelemetryConfig) -> Result<TelemetryGuard> {
    let guard = wakey_core::telemetry::init(
        telemetry,
        "wakey-control-plane",
        match verbose {
            0 => "wakey_control_plane=info,wakey_core=info,opentelemetry=warn",
            1 => "wakey_control_plane=debug,wakey_core=info,opentelemetry=warn",
            _ => "wakey_control_plane=trace,wakey_core=info,opentelemetry=warn",
        },
        None,
    )?;
    #[cfg(target_os = "linux")]
    let guard = guard.with_process_metrics(wakey_linux::telemetry::ProcfsProcessMetrics);
    Ok(guard)
}
