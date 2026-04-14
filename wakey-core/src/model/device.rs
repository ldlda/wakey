use macaddr::MacAddr;
use serde::Serialize;
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

/// MAC-first identifier for a device aggregate.
#[derive(Debug, PartialEq, Eq, Clone, Hash, Serialize)]
pub struct DeviceId {
    #[serde(with = "mac")]
    pub mac: MacAddr,
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
    pub presence: Presence,
}

impl Device {
    /// Merge raw neighbor and DHCP facts into one device aggregate.
    pub fn from_parts(neighbors: Vec<NeighborEntry>, leases: Vec<DhcpLease>) -> Self {
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

        let macs: Vec<MacAddr> = macs.into_iter().collect();
        Self {
            id: macs.first().copied().map(|mac| DeviceId { mac }),
            names: names.into_iter().map(|n| n.to_owned()).collect(),
            ips: ips.into_iter().collect(),
            macs,
            interfaces: interfaces.into_iter().map(|d| d.to_owned()).collect(),
            neighbors,
            leases,
            presence,
        }
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
