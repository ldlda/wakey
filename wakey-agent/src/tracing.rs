use anyhow::Result;
use wakey_core::telemetry::{TelemetryConfig, TelemetryGuard};

pub fn init(
    verbose: u8,
    config: &TelemetryConfig,
    agent_id: Option<&str>,
) -> Result<TelemetryGuard> {
    let guard = wakey_core::telemetry::init(
        config,
        "wakey-agent",
        match verbose {
            0 => "wakey_agent=info,wakey=info,wakey_core=info,opentelemetry=warn",
            1 => "wakey_agent=debug,wakey=debug,wakey_core=info,opentelemetry=warn",
            _ => "wakey_agent=trace,wakey=debug,wakey_core=info,opentelemetry=warn",
        },
        agent_id,
    )?;
    #[cfg(target_os = "linux")]
    let guard = guard.with_process_metrics(wakey_linux::telemetry::ProcfsProcessMetrics);
    Ok(guard)
}
