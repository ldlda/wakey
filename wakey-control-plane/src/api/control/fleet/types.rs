use serde::{Deserialize, Serialize};

use crate::api::commands::RelayCommandResponse;
use crate::state::KnownDeviceSummary;

#[derive(Debug, Default, Deserialize)]
pub struct ListFleetDevicesQuery {
    pub query: Option<String>,
    pub presence: Option<String>,
    pub known: Option<String>,
    pub agent_id: Option<String>,
    pub visibility: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct RefreshFleetDevicesRequest {
    #[serde(default)]
    pub agent_ids: Vec<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RefreshFleetDevicesResponse {
    pub total_accepted: usize,
    pub agents: Vec<RefreshFleetAgentResult>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RefreshFleetAgentResult {
    pub agent_id: String,
    pub status: String,
    pub accepted: usize,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WakeFleetDeviceRequest {
    pub device_key: String,
    pub route_id: Option<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct WakeFleetDeviceResponse {
    pub route: FleetWakeRoute,
    pub command: RelayCommandResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetDevice {
    pub device_key: String,
    pub display_name: String,
    pub known_device: Option<KnownDeviceSummary>,
    pub pinned: bool,
    pub ips: Vec<String>,
    pub macs: Vec<String>,
    pub hostnames: Vec<String>,
    pub agents: Vec<FleetDeviceAgent>,
    pub sources: Vec<String>,
    pub first_seen_unix: Option<u64>,
    pub last_seen_unix: Option<u64>,
    pub presence: String,
    pub route_candidates: Vec<FleetWakeRoute>,
    pub recommended_route: Option<FleetWakeRoute>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetDeviceAgent {
    pub agent_id: String,
    pub nickname: Option<String>,
    pub connected: bool,
    pub last_seen_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetWakeRoute {
    pub route_id: String,
    pub agent_id: String,
    pub nickname: Option<String>,
    pub connected: bool,
    pub mac: Option<String>,
    pub ip: Option<String>,
    pub hostname: Option<String>,
    pub source: String,
    pub last_seen_unix: u64,
    pub wakeable: bool,
}
