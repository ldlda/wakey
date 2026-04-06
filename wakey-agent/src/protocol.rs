use macaddr::MacAddr;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use wakey_core::{
    DeviceFilters, DeviceInventory, DeviceQuery, DhcpLeaseWithState, InterfaceSummary, NeighborEntry,
    Status, WakeResult,
};
use wakey_core::parse::mac;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusRequest {
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

impl StatusRequest {
    pub fn into_device_query(self) -> DeviceQuery {
        if let Some(query) = self.query.as_ref()
            && self.name.is_none()
            && self.ips.is_empty()
            && self.devs.is_empty()
            && self.nuds.is_empty()
            && self.macs.is_empty()
        {
            return DeviceQuery {
                name: Some(query.clone()),
                ..Default::default()
            };
        }

        DeviceQuery {
            name: self.name.or(self.query),
            filter: DeviceFilters {
                ips: self.ips,
                devs: self.devs,
                nuds: self.nuds,
                macs: self.macs,
            },
        }
    }
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
    pub fn into_device_query(self) -> DeviceQuery {
        StatusRequest {
            query: self.query,
            name: self.name,
            ips: self.ips,
            devs: self.devs,
            nuds: self.nuds,
            macs: self.macs,
        }
        .into_device_query()
    }
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
    Status(StatusRequest),
    Leases(LeasesRequest),
    Devs(DevsRequest),
    Inventory(InventoryRequest),
    Wake(WakeRequest),
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandResult {
    Status(Status<NeighborEntry>),
    Leases(Vec<DhcpLeaseWithState>),
    Devs(Vec<InterfaceSummary>),
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
    Result {
        request_id: String,
        result: CommandResult,
    },
    Error {
        request_id: String,
        error: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Command {
        request_id: String,
        command: AgentCommand,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_serialization_is_stable() {
        let msg = ServerMessage::Command {
            request_id: "req-1".into(),
            command: AgentCommand::Leases(LeasesRequest {
                include_state: true,
            }),
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        assert!(json.contains("\"request_id\":\"req-1\""));
        assert!(json.contains("\"kind\":\"leases\""));
    }
}
