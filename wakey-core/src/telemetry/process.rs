//! OS-independent process/host instruments. Callers supply a platform reader.
//! Failed reads omit a sample: zero must not masquerade as a restart or freed RAM.

use opentelemetry::metrics::{Meter, ObservableGauge};
use std::sync::Arc;

/// Values are seconds/bytes, or None when unsupported/unavailable. Readers run
/// on the metric collection thread: keep work small and local, never perform
/// network requests or emit exporter-directed logs from these callbacks.
pub trait ProcessMetricsSource: Send + Sync + 'static {
    fn process_uptime_seconds(&self) -> Option<f64>;
    fn resident_memory_bytes(&self) -> Option<u64>;
    fn host_uptime_seconds(&self) -> Option<f64>;
}

pub(super) struct ProcessMetrics {
    _uptime: ObservableGauge<f64>,
    _resident: ObservableGauge<u64>,
    _host_uptime: ObservableGauge<f64>,
}

impl ProcessMetrics {
    pub(super) fn new(meter: &Meter, source: impl ProcessMetricsSource) -> Self {
        let source = Arc::new(source);
        let uptime = source.clone();
        let resident = source.clone();
        Self {
            _uptime: meter
                .f64_observable_gauge("wakey.process.uptime")
                .with_unit("s")
                .with_description("Process age reported by the platform reader")
                .with_callback(move |observer| {
                    if let Some(seconds) = uptime.process_uptime_seconds()
                        && seconds.is_finite()
                        && seconds >= 0.0
                    {
                        observer.observe(seconds, &[]);
                    }
                })
                .build(),
            _resident: meter
                .u64_observable_gauge("wakey.process.memory.resident")
                .with_unit("By")
                .with_description("Resident process memory reported by the platform reader")
                .with_callback(move |observer| {
                    if let Some(bytes) = resident.resident_memory_bytes() {
                        observer.observe(bytes, &[]);
                    }
                })
                .build(),
            _host_uptime: meter
                .f64_observable_gauge("wakey.host.uptime")
                .with_unit("s")
                .with_description("Host uptime reported by the platform reader")
                .with_callback(move |observer| {
                    if let Some(seconds) = source.host_uptime_seconds()
                        && seconds.is_finite()
                        && seconds >= 0.0
                    {
                        observer.observe(seconds, &[]);
                    }
                })
                .build(),
        }
    }
}
