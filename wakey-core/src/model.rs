use macaddr::MacAddr;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use serde_with::{DisplayFromStr, OneOrMany, serde_as};
use std::{net::IpAddr, str::FromStr};
use strum::{Display, EnumString};

use crate::parse::mac;

#[skip_serializing_none]
#[derive(Debug, PartialEq, Eq, Clone, Hash, Serialize)]
pub struct NeighborEntry {
    pub ip: IpAddr,
    pub dev: Option<String>,
    #[serde(with = "mac::option_mac")]
    pub mac: Option<MacAddr>,
    pub state: NeighborState,
}

#[derive(
    Debug, PartialEq, Eq, EnumString, Display, Clone, Copy, Hash, Serialize, Deserialize, Default,
)]
#[strum(serialize_all = "UPPERCASE", ascii_case_insensitive)]
#[serde(rename_all = "UPPERCASE")]
pub enum NeighborState {
    Permanent,
    Noarp,
    Reachable,
    Stale,
    Incomplete,
    Delay,
    Probe,
    Failed,
    #[serde(other)]
    #[default]
    None,
}

impl NeighborState {
    pub const fn as_ip_neigh_arg(self) -> &'static str {
        match self {
            NeighborState::Permanent => "permanent",
            NeighborState::Reachable => "reachable",
            NeighborState::Stale => "stale",
            NeighborState::Delay => "delay",
            NeighborState::Probe => "probe",
            NeighborState::Incomplete => "incomplete",
            NeighborState::Noarp => "noarp",
            NeighborState::None => "none",
            NeighborState::Failed => "failed",
        }
    }

    pub const fn rank(self) -> u8 {
        match self {
            NeighborState::Permanent | NeighborState::Reachable => 5,
            NeighborState::Stale => 4,
            NeighborState::Delay | NeighborState::Probe | NeighborState::Incomplete => 3,
            NeighborState::Noarp => 2,
            NeighborState::None => 1,
            NeighborState::Failed => 0,
        }
    }
}

impl PartialOrd for NeighborState {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NeighborState {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.rank().cmp(&other.rank())
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash, Serialize, Deserialize, Default)]
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

#[derive(Debug, PartialEq, Eq, Clone, Hash, Serialize)]
pub struct DeviceId {
    #[serde(with = "mac")]
    pub mac: MacAddr,
}

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
            if let Some(name) = &lease.name {
                names.insert(name.clone());
            }
        }
        for neighbor in &neighbors {
            ips.insert(neighbor.ip);
            if let Some(mac) = neighbor.mac {
                macs.insert(mac);
            }
            if let Some(dev) = &neighbor.dev {
                interfaces.insert(dev.clone());
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
            names: names.into_iter().collect(),
            ips: ips.into_iter().collect(),
            macs,
            interfaces: interfaces.into_iter().collect(),
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

#[derive(Debug, Default, Clone, Hash, Deserialize, Serialize)]
pub struct DeviceQuery {
    pub name: Option<String>,
    #[serde(flatten)]
    pub filter: DeviceFilters,
}

#[serde_as]
#[derive(Debug, Default, Clone, Hash, Serialize, Deserialize)]
pub struct DeviceFilters {
    #[serde_as(as = "OneOrMany<_>")]
    #[serde(default)]
    pub ips: Vec<IpAddr>,
    #[serde_as(as = "OneOrMany<_>")]
    #[serde(default)]
    pub devs: Vec<String>,
    #[serde_as(as = "OneOrMany<_>")]
    #[serde(default)]
    pub nuds: Vec<NeighborState>,
    #[serde_as(as = "OneOrMany<DisplayFromStr>")]
    #[serde(default)]
    pub macs: Vec<MacAddr>,
}

#[skip_serializing_none]
#[derive(Debug, Default, Serialize)]
pub struct Status<T> {
    pub name: Option<String>,
    pub table: Vec<T>,
    pub filters: DeviceFilters,
}

#[derive(Debug, Default, Clone, Hash, Deserialize)]
pub struct NamePath {
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DhcpLease {
    pub expires_epoch: u64,
    pub ip: IpAddr,
    #[serde(with = "mac")]
    pub mac: MacAddr,
    pub name: Option<String>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize)]
pub struct DhcpLeaseWithState {
    #[serde(flatten)]
    pub lease_line: DhcpLease,
    pub nud_state: Option<NeighborState>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct LeaseQuery {
    pub include_state: bool,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct DeviceInventory {
    pub devices: Vec<Device>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize)]
pub struct InterfaceSummary {
    pub ifindex: u32,
    pub ifname: String,
    pub operstate: String,
    #[serde(with = "mac::option_mac")]
    pub mac: Option<MacAddr>,
    pub addrs: Vec<InterfaceAddr>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize)]
pub struct InterfaceAddr {
    pub family: Option<String>,
    pub cidr: Option<String>,
    pub broadcast: Option<std::net::Ipv4Addr>,
    pub scope: Option<String>,
    pub label: Option<String>,
}

#[skip_serializing_none]
#[derive(Debug, Serialize, Deserialize, Clone, Copy, Hash, PartialEq, Eq)]
pub struct WakeTarget {
    #[serde(default)]
    pub ip: Option<IpAddr>,
    #[serde(default, with = "mac::option_mac")]
    pub mac: Option<MacAddr>,
}

impl WakeTarget {
    pub const fn is_complete(&self) -> bool {
        matches!(
            self,
            Self {
                ip: Some(_),
                mac: Some(_)
            }
        )
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct WakeResult {
    pub result: Vec<WakeTargetResult>,
}

#[skip_serializing_none]
#[derive(Debug, Serialize, Clone, Copy)]
pub struct WakeTargetResult {
    #[serde(flatten)]
    pub target: WakeTarget,
    pub status: WakeStatus,
}

#[derive(Debug, Serialize, Clone, Copy, Hash, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WakeStatus {
    Succeed,
    NonexistentAddress,
    WrongSize,
    Incomplete,
}

impl WakeTargetResult {
    pub const fn incomplete(target: WakeTarget) -> Self {
        Self {
            target,
            status: WakeStatus::Incomplete,
        }
    }
}

#[derive(Debug)]
pub enum QueryInput {
    Ip(IpAddr),
    Mac(MacAddr),
    Dev(String),
    Nud(NeighborState),
    Name(String),
}

#[derive(Debug, Clone)]
pub enum Query {
    Text(String),
    Ip(IpAddr),
    Mac(MacAddr),
    Interface(String),
    NeighborState(NeighborState),
}

#[derive(Debug, Display, thiserror::Error)]
pub enum NeighborParseError {
    IpWhere,
    IpParseError(#[from] std::net::AddrParseError),
    MacParseError(#[from] macaddr::ParseError),
    StateWhere,
    StateParseError(#[from] strum::ParseError),
}

pub fn parse_neighbor_line(s: &str) -> Result<NeighborEntry, NeighborParseError> {
    let mut it = s.split_whitespace();
    let ip: IpAddr = it.next().ok_or(NeighborParseError::IpWhere)?.parse()?;

    let mut dev: Option<String> = None;
    let mut mac: Option<MacAddr> = None;
    let mut state: Option<NeighborState> = None;
    let mut last_tok: Option<&str> = None;

    while let Some(tok) = it.next() {
        match tok {
            "dev" => dev = it.next().map(str::to_string),
            "lladdr" => {
                mac = it.next().map(|m| m.parse()).transpose()?;
            }
            "nud" => {
                state = it.next().map(|st| st.parse()).transpose()?;
            }
            other => last_tok = Some(other),
        }
    }

    if state.is_none()
        && let Some(st) = last_tok
    {
        state = Some(st.parse()?);
    }

    Ok(NeighborEntry {
        ip,
        dev,
        mac,
        state: state.ok_or(NeighborParseError::StateWhere)?,
    })
}

impl FromStr for NeighborEntry {
    type Err = NeighborParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_neighbor_line(s)
    }
}
