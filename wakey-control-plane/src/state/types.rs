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
