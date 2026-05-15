use std::path::PathBuf;

use macaddr::MacAddr;
use serde::{Deserialize, Serialize};
use wakey_core::{DeviceEndpoint, EndpointKey, EndpointSource, Presence};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssuedAgent {
    pub agent_id: String,
    pub agent_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssuedEnrollToken {
    pub enroll_token: String,
    pub expires_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollTokenInfo {
    pub enroll_token: String,
    pub expires_at_unix: u64,
    pub expired: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateStats {
    pub db_path: PathBuf,
    pub schema_version: u32,
    pub agent_count: usize,
    pub enroll_token_count: usize,
    pub expired_enroll_token_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownDevice {
    pub device_id: String,
    pub display_name: String,
    pub pinned: bool,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
    pub notes: Option<String>,
    pub identifiers: Vec<DeviceIdentifier>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceIdentifier {
    pub identifier_key: String,
    pub device_id: String,
    pub kind: String,
    pub value: String,
    pub created_at_unix: u64,
}

#[derive(Debug, Clone)]
pub struct KnownDeviceInput {
    pub display_name: String,
    pub pinned: bool,
    pub notes: Option<String>,
    pub identifiers: Vec<DeviceIdentifierInput>,
}

#[derive(Debug, Clone)]
pub struct DeviceIdentifierInput {
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownDeviceSummary {
    pub device_id: String,
    pub display_name: String,
    pub pinned: bool,
}

#[derive(Debug, Clone)]
pub struct AgentDeviceRow {
    pub agent_id: String,
    pub device_key: String,
    pub presence: String,
    pub display_name: Option<String>,
    pub first_seen_unix: i64,
    pub last_seen_unix: i64,
}

impl AgentDeviceRow {
    pub fn presence(&self) -> Presence {
        Presence::from(self.presence.as_str())
    }

    pub fn first_seen(&self) -> u64 {
        self.first_seen_unix.max(0) as u64
    }

    pub fn last_seen(&self) -> u64 {
        self.last_seen_unix.max(0) as u64
    }
}

#[derive(Debug, Clone)]
pub struct AgentDeviceMacRow {
    pub agent_id: String,
    pub device_key: String,
    pub mac: String,
}

impl TryFrom<&AgentDeviceMacRow> for MacAddr {
    type Error = macaddr::ParseError;

    fn try_from(row: &AgentDeviceMacRow) -> Result<Self, Self::Error> {
        row.mac.parse()
    }
}

#[derive(Debug, Clone)]
pub struct AgentDeviceIpRow {
    pub agent_id: String,
    pub device_key: String,
    pub ip: String,
}

impl TryFrom<&AgentDeviceIpRow> for std::net::IpAddr {
    type Error = std::net::AddrParseError;

    fn try_from(row: &AgentDeviceIpRow) -> Result<Self, Self::Error> {
        row.ip.parse()
    }
}

#[derive(Debug, Clone)]
pub struct AgentDeviceHostnameRow {
    pub agent_id: String,
    pub device_key: String,
    pub hostname: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AgentDeviceEndpointRow {
    pub agent_id: String,
    pub device_key: String,
    pub endpoint_key: String,
    pub source: String,
    pub mac: Option<String>,
    pub ip: Option<String>,
    pub hostname: Option<String>,
    pub interface: Option<String>,
    pub presence: String,
    pub first_seen_unix: i64,
    pub last_seen_unix: i64,
}

impl AgentDeviceEndpointRow {
    pub fn to_endpoint(&self) -> Option<DeviceEndpoint> {
        let source = endpoint_source_from_str(&self.source)?;
        let mac = match self.mac.as_deref() {
            Some(raw) => match raw.parse() {
                Ok(mac) => Some(mac),
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        endpoint_key = %self.endpoint_key,
                        raw_mac = raw,
                        "failed to parse endpoint mac"
                    );
                    return None;
                }
            },
            None => None,
        };
        let ip = match self.ip.as_deref() {
            Some(raw) => match raw.parse() {
                Ok(ip) => Some(ip),
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        endpoint_key = %self.endpoint_key,
                        raw_ip = raw,
                        "failed to parse endpoint ip"
                    );
                    return None;
                }
            },
            None => None,
        };
        let key = EndpointKey::new(source, mac, ip)?;
        Some(DeviceEndpoint {
            key,
            hostname: self.hostname.clone(),
            interface: self.interface.clone(),
            presence: Presence::from(self.presence.as_str()),
            first_seen_unix: Some(self.first_seen_unix.max(0) as u64),
            last_seen_unix: Some(self.last_seen_unix.max(0) as u64),
        })
    }
}

#[derive(Debug, Clone)]
pub struct AgentDeviceFactRow {
    pub agent_id: String,
    pub device_key: String,
    pub fact_json: String,
}

#[derive(Debug, Clone)]
pub struct AgentDeviceWithChildren {
    pub device: AgentDeviceRow,
    pub macs: Vec<macaddr::MacAddr>,
    pub ips: Vec<std::net::IpAddr>,
    pub hostnames: Vec<String>,
    pub endpoints: Vec<DeviceEndpoint>,
    #[allow(dead_code)]
    pub facts: Vec<String>,
}

fn endpoint_source_from_str(raw: &str) -> Option<EndpointSource> {
    match raw {
        "neighbor" => Some(EndpointSource::Neighbor),
        "dhcp_lease" => Some(EndpointSource::DhcpLease),
        "hook_neighbor" => Some(EndpointSource::HookNeighbor),
        "hook_dhcp" => Some(EndpointSource::HookDhcp),
        _ => {
            tracing::warn!(source = raw, "unknown endpoint source");
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub event_id: String,
    pub ts_unix: u64,
    pub actor_type: String,
    pub actor_id: Option<String>,
    pub agent_id: Option<String>,
    pub request_id: Option<String>,
    pub event_type: String,
    pub outcome: String,
    pub latency_ms: Option<u64>,
    pub message: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct AuditEventInput {
    pub actor_type: String,
    pub actor_id: Option<String>,
    pub agent_id: Option<String>,
    pub request_id: Option<String>,
    pub event_type: String,
    pub outcome: String,
    pub latency_ms: Option<u64>,
    pub message: String,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
pub struct AuditEventFilter {
    pub agent_id: Option<String>,
    pub request_id: Option<String>,
    pub event_type: Option<String>,
    pub outcome: Option<String>,
    pub since_unix: Option<u64>,
    pub until_unix: Option<u64>,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertState {
    pub alert_id: String,
    pub kind: String,
    pub severity: String,
    pub status: String,
    pub agent_id: Option<String>,
    pub message: String,
    pub value: u64,
    pub threshold: u64,
    pub last_seen_unix: u64,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertTransition {
    pub transition_id: String,
    pub ts_unix: u64,
    pub alert_id: String,
    pub kind: String,
    pub agent_id: Option<String>,
    pub from_status: Option<String>,
    pub to_status: String,
    pub message: String,
    pub metadata: serde_json::Value,
}
