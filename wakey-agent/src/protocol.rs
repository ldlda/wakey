use macaddr::MacAddr;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::IpAddr;
use std::time::{Duration, Instant};
use wakey_core::parse::mac;
use wakey_core::{
    Device, DeviceInventory, DhcpLeaseWithState, InterfaceSummary, InventoryQuery,
    InventoryQueryBuilder, WakeResult,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RequestId(String);

impl RequestId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct TerminalId(String);

impl TerminalId {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err("terminal_id must not be empty".into());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TerminalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl TryFrom<String> for TerminalId {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for TerminalId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::try_from(raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentCapability {
    Terminal,
}

pub const DEFAULT_TERMINAL_MAX_SESSIONS: usize = 2;
pub const DEFAULT_TERMINAL_SESSION_TTL_SECONDS: u64 = 12 * 60 * 60;

/// Validates a terminal TTL against the platform's monotonic timer range.
/// Zero is the explicit unlimited policy.
pub fn checked_terminal_session_ttl(
    session_ttl_seconds: u64,
) -> Result<Option<Duration>, &'static str> {
    if session_ttl_seconds == 0 {
        return Ok(None);
    }
    let ttl = Duration::from_secs(session_ttl_seconds);
    Instant::now()
        .checked_add(ttl)
        .ok_or("terminal session TTL is too large for the platform timer")?;
    Ok(Some(ttl))
}

const fn default_terminal_session_ttl_seconds() -> u64 {
    DEFAULT_TERMINAL_SESSION_TTL_SECONDS
}

/// Optional parameters attached to advertised agent capabilities.
///
/// Keep this separate from `AgentCapability`: the capability list remains a
/// compact, backward-compatible feature check, while this object can grow as
/// individual capabilities gain configurable limits or modes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCapabilityOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<TerminalCapabilityOptions>,
}

impl AgentCapabilityOptions {
    fn is_empty(&self) -> bool {
        self.terminal.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalCapabilityOptions {
    pub max_sessions: usize,
    #[serde(default = "default_terminal_session_ttl_seconds")]
    pub session_ttl_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTerminalSession {
    pub terminal_id: TerminalId,
    pub created_at_unix: u64,
    /// The policy captured when this PTY was created. Zero means unlimited.
    #[serde(default = "default_terminal_session_ttl_seconds")]
    pub session_ttl_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TerminalControl {
    Resize { rows: u16, cols: u16 },
    Snapshot,
    Ready,
    Exited { exit_code: Option<i32> },
    Error { code: String, message: String },
    Close,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TerminalAgentHandshake {
    Auth {
        agent_id: String,
        relay_token: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TerminalOperatorHandshake {
    Attach {
        attachment_token: String,
        operator_id: String,
    },
}

impl TryFrom<String> for RequestId {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            return Err("request_id must not be empty".into());
        }
        Ok(Self(value))
    }
}

impl From<RequestId> for String {
    fn from(value: RequestId) -> Self {
        value.0
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for RequestId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RequestId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        RequestId::try_from(raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeasesRequest {
    #[serde(default)]
    pub include_state: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevsRequest {
    pub dev: Option<String>,
    #[serde(default)]
    pub up_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryRequest {
    pub query: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub ips: Vec<IpAddr>,
    #[serde(default)]
    pub devs: Vec<String>,
    #[serde(default)]
    pub nuds: Vec<wakey_core::NeighborState>,
    #[serde(default)]
    #[serde(with = "mac::vec_mac")]
    pub macs: Vec<MacAddr>,
}

impl InventoryRequest {
    pub fn into_inventory_query(self) -> InventoryQuery {
        into_inventory_query(
            self.query, self.name, self.ips, self.devs, self.nuds, self.macs,
        )
    }
}

fn into_inventory_query(
    query: Option<String>,
    name: Option<String>,
    ips: Vec<IpAddr>,
    devs: Vec<String>,
    nuds: Vec<wakey_core::NeighborState>,
    macs: Vec<MacAddr>,
) -> InventoryQuery {
    InventoryQueryBuilder::new()
        .maybe_text(name.or(query))
        .ips(ips)
        .interfaces(devs)
        .neighbor_states(nuds)
        .macs(macs)
        .build()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeRequest {
    pub query: Option<String>,
    #[serde(default)]
    #[serde(with = "mac::option_mac")]
    pub mac: Option<MacAddr>,
    #[serde(default)]
    pub ip: Option<IpAddr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentCommand {
    Leases(LeasesRequest),
    Devs(DevsRequest),
    Inventory(InventoryRequest),
    Wake(WakeRequest),
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandResult {
    Leases { rows: Vec<DhcpLeaseWithState> },
    Devs { rows: Vec<InterfaceSummary> },
    Inventory(DeviceInventory),
    Wake(WakeResult),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Hello {
        agent_id: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        capabilities: Vec<AgentCapability>,
        #[serde(default, skip_serializing_if = "AgentCapabilityOptions::is_empty")]
        capability_options: AgentCapabilityOptions,
    },
    Auth {
        agent_id: String,
        agent_token: String,
    },
    Heartbeat {
        agent_id: String,
    },
    DeviceSnapshot {
        agent_id: String,
        devices: Vec<Device>,
    },
    Result {
        request_id: RequestId,
        result: CommandResult,
    },
    Error {
        request_id: RequestId,
        error: ErrorPayload,
    },
    TerminalRejected {
        terminal_id: TerminalId,
        error: ErrorPayload,
    },
    TerminalSessions {
        sessions: Vec<AgentTerminalSession>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Command {
        request_id: RequestId,
        command: AgentCommand,
    },
    SyncDeviceSnapshot,
    SyncTerminalSessions,
    OpenTerminal {
        terminal_id: TerminalId,
        relay_token: String,
        rows: u16,
        cols: u16,
    },
    CloseTerminal {
        terminal_id: TerminalId,
    },
    ResumeTerminal {
        terminal_id: TerminalId,
        relay_token: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_serialization_is_stable() {
        let msg = ServerMessage::Command {
            request_id: RequestId::try_from("req-1".to_string()).expect("request id"),
            command: AgentCommand::Leases(LeasesRequest {
                include_state: true,
            }),
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        assert!(json.contains("\"request_id\":\"req-1\""));
        assert!(json.contains("\"kind\":\"leases\""));
    }

    #[test]
    fn request_id_rejects_empty() {
        let err = RequestId::try_from("   ".to_string()).expect_err("must fail");
        assert!(err.contains("must not be empty"));
    }

    #[test]
    fn terminal_id_deserialization_enforces_validation() {
        let valid: TerminalId = serde_json::from_str(r#""term-1""#).expect("valid terminal id");
        assert_eq!(valid.as_str(), "term-1");

        let error = serde_json::from_str::<TerminalId>(r#""   ""#)
            .expect_err("empty terminal id must fail");
        assert!(error.to_string().contains("must not be empty"));
    }

    #[test]
    fn client_result_with_devs_serializes() {
        let msg = ClientMessage::Result {
            request_id: RequestId::try_from("req-devs-1".to_string()).expect("request id"),
            result: CommandResult::Devs { rows: vec![] },
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        assert!(json.contains("\"type\":\"result\""));
        assert!(json.contains("\"kind\":\"devs\""));
    }

    #[test]
    fn device_snapshot_serializes() {
        let msg = ClientMessage::DeviceSnapshot {
            agent_id: "agent-a".into(),
            devices: vec![],
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        assert!(json.contains("\"type\":\"device_snapshot\""));
        assert!(json.contains("\"devices\""));
    }

    #[test]
    fn terminal_messages_serialize_with_explicit_frame_types() {
        let message = ServerMessage::OpenTerminal {
            terminal_id: TerminalId::new("term-1").expect("terminal id"),
            relay_token: "secret".into(),
            rows: 30,
            cols: 120,
        };
        let json = serde_json::to_string(&message).expect("serialize open terminal");
        assert!(json.contains("\"type\":\"open_terminal\""));
        assert!(json.contains("\"terminal_id\":\"term-1\""));

        let resize = TerminalControl::Resize {
            rows: 40,
            cols: 160,
        };
        let json = serde_json::to_string(&resize).expect("serialize resize");
        assert_eq!(json, r#"{"type":"resize","rows":40,"cols":160}"#);
        let snapshot =
            serde_json::to_string(&TerminalControl::Snapshot).expect("serialize snapshot");
        assert_eq!(snapshot, r#"{"type":"snapshot"}"#);
        let operator = TerminalOperatorHandshake::Attach {
            attachment_token: "attach-secret".into(),
            operator_id: "browser-tab".into(),
        };
        let json = serde_json::to_string(&operator).expect("serialize operator handshake");
        assert_eq!(
            json,
            r#"{"type":"attach","attachment_token":"attach-secret","operator_id":"browser-tab"}"#
        );

        let inventory = ClientMessage::TerminalSessions {
            sessions: vec![AgentTerminalSession {
                terminal_id: TerminalId::new("term-1").expect("terminal id"),
                created_at_unix: 42,
                session_ttl_seconds: 600,
            }],
        };
        let json = serde_json::to_string(&inventory).expect("serialize terminal inventory");
        assert!(json.contains("\"type\":\"terminal_sessions\""));
        assert!(json.contains("\"created_at_unix\":42"));

        let resume = ServerMessage::ResumeTerminal {
            terminal_id: TerminalId::new("term-1").expect("terminal id"),
            relay_token: "replacement".into(),
        };
        let json = serde_json::to_string(&resume).expect("serialize terminal resume");
        assert!(json.contains("\"type\":\"resume_terminal\""));
    }

    #[test]
    fn hello_serializes_typed_capability_options() {
        let message = ClientMessage::Hello {
            agent_id: "router".into(),
            capabilities: vec![AgentCapability::Terminal],
            capability_options: AgentCapabilityOptions {
                terminal: Some(TerminalCapabilityOptions {
                    max_sessions: 3,
                    session_ttl_seconds: 600,
                }),
            },
        };

        let value = serde_json::to_value(message).expect("serialize hello");
        assert_eq!(value["capability_options"]["terminal"]["max_sessions"], 3);
        assert_eq!(
            value["capability_options"]["terminal"]["session_ttl_seconds"],
            600
        );
    }

    #[test]
    fn terminal_ttl_defaults_for_old_messages_and_preserves_explicit_zero() {
        let old_options: TerminalCapabilityOptions =
            serde_json::from_value(serde_json::json!({ "max_sessions": 2 }))
                .expect("deserialize old capability options");
        assert_eq!(
            old_options.session_ttl_seconds,
            DEFAULT_TERMINAL_SESSION_TTL_SECONDS
        );

        let old_session: AgentTerminalSession = serde_json::from_value(serde_json::json!({
            "terminal_id": "term-old",
            "created_at_unix": 42
        }))
        .expect("deserialize old terminal inventory");
        assert_eq!(
            old_session.session_ttl_seconds,
            DEFAULT_TERMINAL_SESSION_TTL_SECONDS
        );

        let unlimited: TerminalCapabilityOptions = serde_json::from_value(serde_json::json!({
            "max_sessions": 2,
            "session_ttl_seconds": 0
        }))
        .expect("deserialize unlimited capability options");
        assert_eq!(unlimited.session_ttl_seconds, 0);
    }
}
