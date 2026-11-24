use serde::{Deserialize, Serialize};

/// Best-effort values for one process and the host visible to it.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ProcessStatus {
    pub process_uptime_seconds: Option<f64>,
    pub resident_memory_bytes: Option<u64>,
    pub cpu_time_seconds: Option<f64>,
    pub open_file_descriptors: Option<u64>,
    pub host_uptime_seconds: Option<f64>,
    pub host_memory_total_bytes: Option<u64>,
    pub host_memory_available_bytes: Option<u64>,
    pub host_load_1m: Option<f64>,
}
