use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use opentelemetry::{KeyValue, metrics::ObservableGauge};

/// Retained CC rows, not unique addresses or online-device counts.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AgentDeviceCounts {
    pub devices: u64,
    pub endpoints: u64,
}

/// One unfiltered, revision-checked CC sample. Agent IDs are the only identity labels.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct FleetCounts {
    pub agents: BTreeMap<String, AgentDeviceCounts>,
    pub saved: u64,
    pub unsaved: u64,
}

/// Callbacks only read memory; async DB work stays outside the export thread.
/// Replacing the entire sample removes vanished labels instead of retaining their series.
pub struct FleetMetrics {
    sample: Arc<RwLock<Option<FleetCounts>>>,
    _devices: ObservableGauge<u64>,
    _endpoints: ObservableGauge<u64>,
    _fleet: ObservableGauge<u64>,
}

impl FleetMetrics {
    /// Avoid polling the DB at all when OTLP metrics are disabled.
    pub fn new() -> Option<Self> {
        super::METRICS.get()?;
        let meter = opentelemetry::global::meter("wakey");
        let sample = Arc::new(RwLock::new(None::<FleetCounts>));
        let devices = sample.clone();
        let endpoints = sample.clone();
        let fleet = sample.clone();
        Some(Self {
            sample,
            _devices: meter
                .u64_observable_gauge("wakey.stored.devices")
                .with_callback(move |observer| {
                    if let Ok(sample) = devices.read()
                        && let Some(sample) = sample.as_ref()
                    {
                        for (agent, counts) in &sample.agents {
                            observer.observe(
                                counts.devices,
                                &[KeyValue::new("agent_id", agent.clone())],
                            );
                        }
                    }
                })
                .build(),
            _endpoints: meter
                .u64_observable_gauge("wakey.stored.endpoints")
                .with_callback(move |observer| {
                    if let Ok(sample) = endpoints.read()
                        && let Some(sample) = sample.as_ref()
                    {
                        for (agent, counts) in &sample.agents {
                            observer.observe(
                                counts.endpoints,
                                &[KeyValue::new("agent_id", agent.clone())],
                            );
                        }
                    }
                })
                .build(),
            _fleet: meter
                .u64_observable_gauge("wakey.fleet.devices")
                .with_callback(move |observer| {
                    if let Ok(sample) = fleet.read()
                        && let Some(sample) = sample.as_ref()
                    {
                        observer.observe(sample.saved, &[KeyValue::new("identity", "saved")]);
                        observer.observe(sample.unsaved, &[KeyValue::new("identity", "unsaved")]);
                    }
                })
                .build(),
        })
    }

    /// None suppresses observations after a failed refresh, rather than reporting stale counts.
    pub fn replace(&self, sample: Option<FleetCounts>) {
        if let Ok(mut current) = self.sample.write() {
            *current = sample;
        }
    }
}
