use macaddr::MacAddr;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::net::IpAddr;

use crate::model::{DhcpLease, NeighborEntry, NeighborState};
use crate::parse::mac;

/// Product-level presence derived from raw neighbor state.
#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash, Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    Online,
    LikelyOnline,
    #[default]
    Unknown,
    Offline,
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

/// One raw source fact used while building a device aggregate.
///
/// These are intentionally source-shaped and non-durable. They preserve details
/// from hooks and live inventory so higher layers can explain why a device looks
/// online, stale, or unknown without reverse-engineering flattened fields.
#[skip_serializing_none]
#[derive(Debug, PartialEq, Eq, Clone, Hash, Serialize)]
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
#[derive(Debug, Clone, Serialize)]
pub struct Device {
    pub id: Option<DeviceId>,
    pub names: Vec<String>,
    pub ips: Vec<IpAddr>,
    #[serde(with = "mac::vec_mac")]
    pub macs: Vec<MacAddr>,
    pub interfaces: Vec<String>,
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
        let mut presence = Presence::Unknown;

        for lease in &leases {
            ips.insert(lease.ip);
            macs.insert(lease.mac);
            if let Some(name) = lease.name.as_deref() {
                names.insert(name);
            }
        }
        for neighbor in &neighbors {
            ips.insert(neighbor.ip);
            if let Some(mac) = neighbor.mac {
                macs.insert(mac);
            }
            if let Some(dev) = neighbor.dev.as_deref() {
                interfaces.insert(dev);
            }
            presence = std::cmp::max(
                presence_rank(presence),
                presence_rank(neighbor.state.into()),
            )
            .into();
        }
        let mut observed_non_remove = false;
        for observation in &observations {
            if let Some(name) = observation.hostname.as_deref() {
                names.insert(name);
            }
            if let Some(ip) = observation.ip {
                ips.insert(ip);
            }
            if let Some(mac) = observation.mac {
                macs.insert(mac);
            }
            if observation.action != "remove" {
                observed_non_remove = true;
            }
            presence = std::cmp::max(
                presence_rank(presence),
                presence_rank(observation_presence(observation)),
            )
            .into();
        }

        if neighbors.is_empty()
            && leases.is_empty()
            && !observed_non_remove
            && !observations.is_empty()
        {
            presence = Presence::Offline;
        }

        let macs: Vec<MacAddr> = macs.into_iter().collect();
        let ips: Vec<IpAddr> = ips.into_iter().collect();
        let id = macs
            .first()
            .copied()
            .map(DeviceId::Mac)
            .or_else(|| ips.first().copied().map(DeviceId::Ip));
        Self {
            id,
            names: names.into_iter().map(|n| n.to_owned()).collect(),
            ips,
            macs,
            interfaces: interfaces.into_iter().map(|d| d.to_owned()).collect(),
            neighbors,
            leases,
            observations,
            presence,
        }
    }
}

fn observation_presence(observation: &DeviceObservationFact) -> Presence {
    match (observation.kind.as_str(), observation.action.as_str()) {
        (_, "remove" | "failed") => Presence::Offline,
        (_, "permanent" | "reachable") => Presence::Online,
        (_, "stale") => Presence::LikelyOnline,
        ("neigh", "add" | "update" | "old") => Presence::LikelyOnline,
        _ => Presence::Unknown,
    }
}

const fn presence_rank(presence: Presence) -> u8 {
    match presence {
        Presence::Online => 3,
        Presence::LikelyOnline => 2,
        Presence::Unknown => 1,
        Presence::Offline => 0,
    }
}

impl From<u8> for Presence {
    fn from(value: u8) -> Self {
        match value {
            3 => Self::Online,
            2 => Self::LikelyOnline,
            0 => Self::Offline,
            _ => Self::Unknown,
        }
    }
}

/// Collection of merged discovered devices.
#[derive(Debug, Default, Clone, Serialize)]
pub struct DeviceInventory {
    pub devices: Vec<Device>,
}
