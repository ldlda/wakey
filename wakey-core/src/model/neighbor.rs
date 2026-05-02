use macaddr::MacAddr;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::{net::IpAddr, str::FromStr};
use strum::{Display, EnumString};

use crate::parse::mac;

/// One neighbor-table row, typically derived from `ip neigh` or netlink.
#[skip_serializing_none]
#[derive(Debug, PartialEq, Eq, Clone, Hash, Serialize, Deserialize)]
pub struct NeighborEntry {
    pub ip: IpAddr,
    pub dev: Option<String>,
    #[serde(with = "mac::option_mac")]
    pub mac: Option<MacAddr>,
    pub state: NeighborState,
}

/// Linux neighbor reachability state.
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
    /// Lowercase CLI argument form used by `ip neigh`.
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

    /// Ordering rank used when choosing the “best” state among multiple rows.
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

/// Errors returned when parsing a text `ip neigh` line.
#[derive(Debug, Display, thiserror::Error)]
pub enum NeighborParseError {
    IpWhere,
    IpParseError(#[from] std::net::AddrParseError),
    MacParseError(#[from] macaddr::ParseError),
    StateWhere,
    StateParseError(#[from] strum::ParseError),
}

/// Parse one textual `ip neigh` line into a typed neighbor row.
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
