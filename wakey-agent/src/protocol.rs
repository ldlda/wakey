use macaddr::MacAddr;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::IpAddr;
use wakey_core::parse::mac;
use wakey_core::{
    DeviceInventory, DhcpLeaseWithState, InterfaceSummary, InventoryQuery, InventoryQueryBuilder,
    WakeResult,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RequestId(String);

impl RequestId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
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
pub struct AgentObservation {
    pub kind: String,
    pub action: String,
    pub mac: Option<String>,
    pub ip: Option<IpAddr>,
    pub hostname: Option<String>,
    pub first_seen_unix: u64,
    pub last_seen_unix: u64,
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
    },
    Auth {
        agent_id: String,
        agent_token: String,
    },
    Heartbeat {
        agent_id: String,
    },
    Observations {
        agent_id: String,
        observations: Vec<AgentObservation>,
    },
    Result {
        request_id: RequestId,
        result: CommandResult,
    },
    Error {
        request_id: RequestId,
        error: ErrorPayload,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Command {
        request_id: RequestId,
        command: AgentCommand,
    },
    SyncObservations,
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
    fn observations_message_serializes() {
        let msg = ClientMessage::Observations {
            agent_id: "agent-a".into(),
            observations: vec![AgentObservation {
                kind: "dhcp".into(),
                action: "update".into(),
                mac: Some("aa:bb:cc:dd:ee:ff".into()),
                ip: Some("192.168.1.10".parse().expect("ip")),
                hostname: Some("lda".into()),
                first_seen_unix: 10,
                last_seen_unix: 20,
            }],
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        assert!(json.contains("\"type\":\"observations\""));
        assert!(json.contains("\"kind\":\"dhcp\""));
    }
}
