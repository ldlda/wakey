use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
pub struct AgentDeviceObservation {
    pub observation_key: String,
    pub agent_id: String,
    pub kind: String,
    pub mac: Option<String>,
    pub ip: Option<String>,
    pub hostname: Option<String>,
    pub first_seen_unix: u64,
    pub last_seen_unix: u64,
    pub last_action: String,
}

#[derive(Debug, Clone)]
pub struct AgentDeviceObservationInput {
    pub kind: String,
    pub action: String,
    pub mac: Option<String>,
    pub ip: Option<String>,
    pub hostname: Option<String>,
    pub first_seen_unix: u64,
    pub last_seen_unix: u64,
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
