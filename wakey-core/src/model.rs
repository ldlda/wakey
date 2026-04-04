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
