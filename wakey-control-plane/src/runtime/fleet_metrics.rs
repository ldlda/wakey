use std::time::Duration;

use wakey_core::telemetry::FleetMetrics;

use super::AppState;

/// Tie polling to the daemon lifetime, including early server errors.
pub(super) struct FleetMetricsTask(tokio::task::JoinHandle<()>);

impl FleetMetricsTask {
    pub(super) fn start(state: AppState) -> Option<Self> {
        let metrics = FleetMetrics::new()?;
        Some(Self(tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                match crate::api::sample_fleet_counts(&state).await {
                    Ok(sample) => metrics.replace(Some(sample)),
                    Err(error) => {
                        metrics.replace(None);
                        tracing::warn!(%error, "failed to refresh fleet metrics");
                    }
                }
            }
        })))
    }
}

impl Drop for FleetMetricsTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}
