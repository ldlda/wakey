//! OS-independent process/host instruments. Callers supply a platform reader.
//! Failed reads omit a sample: zero must not masquerade as a restart or freed RAM.

use opentelemetry::KeyValue;
use opentelemetry::metrics::{Meter, ObservableCounter, ObservableGauge};
use std::sync::Arc;

/// Host-visible memory in bytes; available is an estimate, not merely free RAM.
pub struct HostMemory {
    pub total_bytes: u64,
    pub available_bytes: Option<u64>,
}

/// Host load averages, not CPU utilization percentages.
pub struct HostLoad {
    pub one_minute: f64,
    pub five_minutes: f64,
    pub fifteen_minutes: f64,
}

/// Values use the documented seconds/bytes/count units, or None when unavailable. Readers run
/// on the metric collection thread: keep work small and local, never perform
/// network requests or emit exporter-directed logs from these callbacks.
pub trait ProcessMetricsSource: Send + Sync + 'static {
    fn process_uptime_seconds(&self) -> Option<f64>;
    fn resident_memory_bytes(&self) -> Option<u64>;
    fn host_uptime_seconds(&self) -> Option<f64>;
    /// Cumulative user + system CPU seconds for the process, excluding children.
    /// Optional additions default to unavailable for existing platform readers.
    fn cpu_time_seconds(&self) -> Option<f64> {
        None
    }
    fn open_file_descriptors(&self) -> Option<u64> {
        None
    }
    fn host_memory(&self) -> Option<HostMemory> {
        None
    }
    fn host_load(&self) -> Option<HostLoad> {
        None
    }
}

pub(super) struct ProcessMetrics {
    _uptime: ObservableGauge<f64>,
    _resident: ObservableGauge<u64>,
    _host_uptime: ObservableGauge<f64>,
    _cpu_time: ObservableCounter<f64>,
    _open_fds: ObservableGauge<u64>,
    _host_total_memory: ObservableGauge<u64>,
    _host_available_memory: ObservableGauge<u64>,
    _host_load: ObservableGauge<f64>,
}

impl ProcessMetrics {
    pub(super) fn new(meter: &Meter, source: impl ProcessMetricsSource) -> Self {
        let source = Arc::new(source);
        let uptime = source.clone();
        let resident = source.clone();
        let host = source.clone();
        let cpu = source.clone();
        let fds = source.clone();
        let total_memory = source.clone();
        let available_memory = source.clone();
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
                    if let Some(seconds) = host.host_uptime_seconds()
                        && seconds.is_finite()
                        && seconds >= 0.0
                    {
                        observer.observe(seconds, &[]);
                    }
                })
                .build(),
            _cpu_time: meter
                .f64_observable_counter("wakey.process.cpu.time")
                .with_unit("s")
                .with_description("Cumulative user and system process CPU time, excluding children")
                .with_callback(move |observer| {
                    if let Some(seconds) = cpu.cpu_time_seconds()
                        && seconds.is_finite()
                        && seconds >= 0.0
                    {
                        observer.observe(seconds, &[]);
                    }
                })
                .build(),
            _open_fds: meter
                .u64_observable_gauge("wakey.process.open_fds")
                .with_description(
                    "Current open file-descriptor count reported by the platform reader",
                )
                .with_callback(move |observer| {
                    if let Some(count) = fds.open_file_descriptors() {
                        observer.observe(count, &[]);
                    }
                })
                .build(),
            _host_total_memory: meter
                .u64_observable_gauge("wakey.host.memory.total")
                .with_unit("By")
                .with_description("Total usable RAM reported by the platform reader")
                .with_callback(move |observer| {
                    if let Some(memory) = total_memory.host_memory() {
                        observer.observe(memory.total_bytes, &[]);
                    }
                })
                .build(),
            _host_available_memory: meter
                .u64_observable_gauge("wakey.host.memory.available")
                .with_unit("By")
                .with_description("Estimated RAM available for applications without swapping")
                .with_callback(move |observer| {
                    if let Some(memory) = available_memory.host_memory()
                        && let Some(bytes) = memory.available_bytes
                    {
                        observer.observe(bytes, &[]);
                    }
                })
                .build(),
            _host_load: meter
                .f64_observable_gauge("wakey.host.load")
                .with_description("Host load average over the labeled period, not CPU percent")
                .with_callback(move |observer| {
                    if let Some(load) = source.host_load() {
                        for (period, value) in [
                            ("1m", load.one_minute),
                            ("5m", load.five_minutes),
                            ("15m", load.fifteen_minutes),
                        ] {
                            if value.is_finite() && value >= 0.0 {
                                observer.observe(value, &[KeyValue::new("period", period)]);
                            }
                        }
                    }
                })
                .build(),
        }
    }
}
