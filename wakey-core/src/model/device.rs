use macaddr::MacAddr;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::net::IpAddr;

use crate::model::{DhcpLease, NeighborEntry, NeighborState};
use crate::parse::mac;

/// Product-level presence derived from raw neighbor state.
///
/// Variant order defines `Ord`: Unknown < Offline < LikelyOnline < Online.
/// `std::cmp::max` picks the most-online signal when merging.
#[derive(
    Debug, PartialEq, Eq, Clone, Copy, Hash, Ord, PartialOrd, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    #[default]
    Unknown,
    Offline,
    LikelyOnline,
    Online,
}

impl Presence {
    pub fn as_str(&self) -> &'static str {
        match self {
            Presence::Online => "online",
            Presence::LikelyOnline => "likely_online",
            Presence::Unknown => "unknown",
            Presence::Offline => "offline",
        }
    }
}

impl From<&str> for Presence {
    fn from(s: &str) -> Self {
        match s {
            "online" => Presence::Online,
            "likely_online" => Presence::LikelyOnline,
            "offline" => Presence::Offline,
            _ => Presence::Unknown,
        }
    }
}

impl From<NeighborState> for Presence {
    fn from(value: NeighborState) -> Self {
        match value {
            NeighborState::Permanent | NeighborState::Reachable => Self::Online,
            NeighborState::Stale => Self::LikelyOnline,
            NeighborState::Failed => Self::Offline,
            NeighborState::Delay
            | NeighborState::Probe
            | NeighborState::Incomplete
            | NeighborState::Noarp
            | NeighborState::None => Self::Unknown,
        }
    }
}

/// Observed identifier for a discovered device aggregate.
///
/// This is not a durable, user-approved identity. The control plane may attach
/// many observed identifiers to one saved device.
#[derive(Debug, PartialEq, Eq, Clone, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum DeviceId {
    #[serde(with = "mac")]
    Mac(MacAddr),
    Ip(IpAddr),
}

/// Typed origin of an endpoint.
///
/// Raw source strings stay on `DeviceObservationFact` for debugging. Endpoint
/// source is the domain value used for summaries and wake-route ranking.
#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointSource {
    Neighbor,
    DhcpLease,
    HookNeighbor,
    HookDhcp,
}

impl EndpointSource {
    /// Whether this source contributes an IP to the device summary.
    pub const fn summary_ip_eligible(self) -> bool {
        matches!(self, Self::Neighbor | Self::DhcpLease)
    }

    /// Source quality used after reachability and recency when ranking routes.
    pub const fn quality_rank(self) -> u8 {
        match self {
            Self::Neighbor => 4,
            Self::DhcpLease => 3,
            Self::HookNeighbor => 2,
            Self::HookDhcp => 1,
        }
    }
}

/// Source-scoped identity for one observed network contact point.
#[skip_serializing_none]
#[derive(Debug, PartialEq, Eq, Clone, Hash, Serialize, Deserialize)]
pub struct EndpointKey {
    pub source: EndpointSource,
    #[serde(with = "mac::option_mac", default)]
    pub mac: Option<MacAddr>,
    pub ip: Option<IpAddr>,
}

impl EndpointKey {
    pub fn new(source: EndpointSource, mac: Option<MacAddr>, ip: Option<IpAddr>) -> Option<Self> {
        if mac.is_none() && ip.is_none() {
            return None;
        }
        Some(Self { source, mac, ip })
    }
}

/// Agent-scoped endpoint key used by control-plane storage and APIs.
#[derive(Debug, PartialEq, Eq, Clone, Hash, Serialize, Deserialize)]
pub struct AgentEndpointKey {
    pub agent_id: String,
    #[serde(flatten)]
    pub endpoint: EndpointKey,
}

/// One interpreted network contact point for a device aggregate.
#[skip_serializing_none]
#[derive(Debug, PartialEq, Eq, Clone, Hash, Serialize, Deserialize)]
pub struct DeviceEndpoint {
    pub key: EndpointKey,
    pub hostname: Option<String>,
    pub interface: Option<String>,
    pub presence: Presence,
    pub first_seen_unix: Option<u64>,
    pub last_seen_unix: Option<u64>,
}

/// One raw source fact used while building a device aggregate.
///
/// These are intentionally source-shaped and non-durable. They preserve details
/// from hooks (arp, dhcp, etc) so higher layers can explain why a device looks
/// online, stale, or unknown.
#[skip_serializing_none]
#[derive(Debug, PartialEq, Eq, Clone, Hash, Serialize, Deserialize)]
pub struct DeviceObservationFact {
    pub kind: String,
    pub action: String,
    #[serde(with = "mac::option_mac")]
    pub mac: Option<MacAddr>,
    pub ip: Option<IpAddr>,
    pub hostname: Option<String>,
    pub first_seen_unix: Option<u64>,
    pub last_seen_unix: Option<u64>,
}

/// Merged view of one discovered network identity.
///
/// This aggregates facts from DHCP leases and neighbor-table rows into a more
/// useful application-level shape.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub id: Option<DeviceId>,
    pub names: Vec<String>,
    pub ips: Vec<IpAddr>,
    #[serde(with = "mac::vec_mac")]
    pub macs: Vec<MacAddr>,
    pub interfaces: Vec<String>,
    #[serde(default)]
    pub endpoints: Vec<DeviceEndpoint>,
    pub neighbors: Vec<NeighborEntry>,
    pub leases: Vec<DhcpLease>,
    pub observations: Vec<DeviceObservationFact>,
    pub presence: Presence,
}

impl Device {
    /// Merge raw neighbor and DHCP facts into one device aggregate.
    pub fn from_parts(neighbors: Vec<NeighborEntry>, leases: Vec<DhcpLease>) -> Self {
        Self::from_parts_with_observations(neighbors, leases, Vec::new())
    }

    /// Merge raw neighbor, DHCP, and hook observation facts into one aggregate.
    pub fn from_parts_with_observations(
        neighbors: Vec<NeighborEntry>,
        leases: Vec<DhcpLease>,
        observations: Vec<DeviceObservationFact>,
    ) -> Self {
        use std::collections::BTreeSet;

        let mut names = BTreeSet::new();
        let mut ips = BTreeSet::new();
        let mut macs = BTreeSet::new();
        let mut interfaces = BTreeSet::new();
        let mut endpoints = Vec::new();
        let mut presence = Presence::Unknown;

        for lease in &leases {
            ips.insert(lease.ip);
            macs.insert(lease.mac);
            if let Some(name) = lease.name.as_deref() {
                names.insert(name);
            }
            endpoints.push(DeviceEndpoint {
                key: EndpointKey {
                    source: EndpointSource::DhcpLease,
                    mac: Some(lease.mac),
                    ip: Some(lease.ip),
                },
                hostname: lease.name.clone(),
                interface: None,
                presence: Presence::Unknown,
                first_seen_unix: None,
                last_seen_unix: None,
            });
        }
        for neighbor in &neighbors {
            ips.insert(neighbor.ip);
            if let Some(mac) = neighbor.mac {
                macs.insert(mac);
            }
            if let Some(dev) = neighbor.dev.as_deref() {
                interfaces.insert(dev);
            }
            let endpoint_presence = Presence::from(neighbor.state);
            endpoints.push(DeviceEndpoint {
                key: EndpointKey {
                    source: EndpointSource::Neighbor,
                    mac: neighbor.mac,
                    ip: Some(neighbor.ip),
                },
                hostname: None,
                interface: neighbor.dev.clone(),
                presence: endpoint_presence,
                first_seen_unix: None,
                last_seen_unix: None,
            });
            presence = std::cmp::max(presence, endpoint_presence);
        }

        for observation in &observations {
            if let Some(name) = observation.hostname.as_deref() {
                names.insert(name);
            }
            if let Some(mac) = observation.mac {
                macs.insert(mac);
            }
            if let Some(source) = observation_endpoint_source(observation)
                && let Some(key) = EndpointKey::new(source, observation.mac, observation.ip)
            {
                let endpoint_presence = observation_presence(observation);
                endpoints.push(DeviceEndpoint {
                    key,
                    hostname: observation.hostname.clone(),
                    interface: None,
                    presence: endpoint_presence,
                    first_seen_unix: observation.first_seen_unix,
                    last_seen_unix: observation.last_seen_unix,
                });
                presence = std::cmp::max(presence, endpoint_presence);
            }
        }

        let macs: Vec<MacAddr> = macs.into_iter().collect();
        let ips: Vec<IpAddr> = ips.into_iter().collect();
        let id = macs.first().copied().map(DeviceId::Mac).or_else(|| {
            endpoints
                .iter()
                .filter_map(|endpoint| endpoint.key.ip)
                .min()
                .map(DeviceId::Ip)
        });
        Self {
            id,
            names: names.into_iter().map(|n| n.to_owned()).collect(),
            ips,
            macs,
            interfaces: interfaces.into_iter().map(|d| d.to_owned()).collect(),
            endpoints,
            neighbors,
            leases,
            observations,
            presence,
        }
    }
}

fn observation_presence(observation: &DeviceObservationFact) -> Presence {
    match (observation.kind.as_str(), observation.action.as_str()) {
        ("neigh", "remove") => Presence::Offline,
        ("neigh", "add" | "update" | "old") => Presence::LikelyOnline,
        _ => Presence::Unknown,
    }
}

fn observation_endpoint_source(observation: &DeviceObservationFact) -> Option<EndpointSource> {
    match observation.kind.as_str() {
        "neigh" => Some(EndpointSource::HookNeighbor),
        "dhcp" => Some(EndpointSource::HookDhcp),
        _ => None,
    }
}

/// Collection of merged discovered devices.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DeviceInventory {
    #[serde(default)]
    pub devices: Vec<Device>,
}
