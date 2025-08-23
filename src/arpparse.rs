// struct arp;

// async fn read_arp() -> io::Result<()> {
//     let arp_file = tokio::fs::File::open("/proc/net/arp").await?;
//     let arp_read = BufReader::new(arp_file);
//     Ok(())
// }

//! ip neigh pass

use std::{net::IpAddr, str::FromStr};

use macaddr::MacAddr;
use serde::{Deserialize, Serialize, Serializer, de};
use serde_with::skip_serializing_none;
use strum::{Display, EnumString};

use crate::arpparse::error::IPNeighParseError;
mod error;
/// ip neigh has some cool shit.
/// IP
/// dev DEV | None
/// lladdr MAC | None
/// status  { permanent | noarp | stale | reachable | none | incomplete | delay | probe | failed } (ip neigh help)
/// so you can see its damn good
///
#[skip_serializing_none]
#[derive(Debug, PartialEq, Eq, Clone, Hash, serde::Serialize)]
pub struct IpNeighLine {
    pub ip: IpAddr,
    pub dev: Option<String>,
    /// link layer address
    #[serde(serialize_with = "ser_opm")]
    pub mac: Option<MacAddr>,
    /// Neighbour Unreachability    Detection
    pub state: NUDState,
}

pub fn ser_opm<S: Serializer>(bro: &Option<MacAddr>, ser: S) -> Result<S::Ok, S::Error> {
    Option::<String>::serialize(&bro.as_ref().map(ToString::to_string), ser)
}

pub fn des_opm<'de, D>(des: D) -> Result<Option<MacAddr>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<&str>::deserialize(des)?
        .map(MacAddr::from_str)
        .transpose()
        .map_err(de::Error::custom)
}

#[derive(Debug, PartialEq, Eq, EnumString, Display, Clone, Copy, Hash, serde::Serialize)]
#[strum(serialize_all = "UPPERCASE")]
#[serde(rename_all = "UPPERCASE")]
pub enum NUDState {
    /// the neighbour entry is valid forever and can
    /// be only be removed administratively.
    Permanent,

    /// the neighbour entry is valid. No attempts to
    /// validate this entry will be made but it can
    /// be removed when its lifetime expires.
    Noarp,

    /// the neighbour entry is valid until the
    /// reachability timeout expires.
    Reachable,

    /// the neighbour entry is valid but suspicious.
    /// This option to ip neigh does not change the
    /// neighbour state if it was valid and the
    /// address is not changed by this command.
    Stale,
    /// this is a pseudo state used when initially
    /// creating a neighbour entry or after trying to
    /// remove it before it becomes free to do so.
    None,

    /// the neighbour entry has not (yet) been
    /// validated/resolved.
    Incomplete,
    /// neighbor entry validation is currently
    /// delayed.
    Delay,
    /// neighbor is being probed.
    Probe,
    /// max number of probes exceeded without
    /// success, neighbor validation has ultimately
    /// failed.
    Failed,
}

impl NUDState {
    /// dumb UI label
    pub fn dumber_state(&self) -> &'static str {
        match self {
            NUDState::Permanent | NUDState::Reachable => "online",
            NUDState::Stale => "maybe online",
            NUDState::Delay | NUDState::Probe | NUDState::Incomplete => "resolving",
            NUDState::Noarp => "static",
            NUDState::None => "unknown",
            NUDState::Failed => "offline",
        }
    }
    /// dumb boolean: Some(true)=on, Some(false)=off, None=shrug
    pub fn dumber_state_this_way(&self) -> Option<bool> {
        match self {
            NUDState::Permanent | NUDState::Reachable => Some(true),
            NUDState::Failed => Some(false),
            _ => None,
        }
    }
}
// thanks copilot for the PEAK
pub fn parse_ip_neigh_line(s: &str) -> Result<IpNeighLine, IPNeighParseError> {
    let mut it = s.split_whitespace();
    let ip: IpAddr = it.next().ok_or(IPNeighParseError::IpWhere)?.parse()?;

    let mut dev: Option<String> = None;
    let mut mac: Option<MacAddr> = None;
    let mut state: Option<NUDState> = None;
    let mut last_tok: Option<&str> = None;

    while let Some(tok) = it.next() {
        match tok {
            "dev" => dev = it.next().map(str::to_string),
            "lladdr" => {
                mac = it.next().map(|m| m.parse()).transpose()?;
            }
            "nud" => {
                // we know this aint happening
                state = it.next().map(|st| st.parse()).transpose()?;
            }
            other => last_tok = Some(other),
        }
    }

    // If no explicit "nud", many outputs end with STATE
    if state.is_none()
        && let Some(st) = last_tok
    {
        state = Some(st.parse()?);
    }

    Ok(IpNeighLine {
        ip,
        dev,
        mac,
        state: state.ok_or(IPNeighParseError::StateWhere)?,
    })
}

impl FromStr for IpNeighLine {
    type Err = IPNeighParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_ip_neigh_line(s)
    }
}
/// pls dont touch ts
impl IpNeighLine {
    pub fn set_ip(&mut self, ip: IpAddr) {
        self.ip = ip;
    }
    pub fn set_state(&mut self, state: NUDState) {
        self.state = state;
    }
    pub fn ip(self, ip: IpAddr) -> Self {
        Self { ip, ..self }
    }
    pub fn state(self, state: NUDState) -> Self {
        Self { state, ..self }
    }
}

// ideas from copilot:

impl NUDState {
    // higher is "better"/more online
    pub const fn rank(self) -> u8 {
        match self {
            NUDState::Permanent | NUDState::Reachable => 5,
            NUDState::Stale => 4,
            NUDState::Delay | NUDState::Probe | NUDState::Incomplete => 3,
            NUDState::Noarp => 2,
            NUDState::None => 1,
            NUDState::Failed => 0,
        }
    }
}
impl PartialOrd for NUDState {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for NUDState {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.rank().cmp(&other.rank())
    }
}

impl IpNeighLine {
    // score for “local and online”: state, has-mac, v4, iface preference
    pub fn score(&self) -> (u8, u8, u8, u8) {
        let iface = self
            .dev
            .as_deref()
            .map(|d| {
                if d.starts_with("br") || d.starts_with("lan") || d.starts_with("eth") {
                    2
                } else if d.starts_with("wlan") || d.starts_with("wl") {
                    1
                } else {
                    0
                }
            })
            .unwrap_or(0);
        (
            self.state.rank(),
            self.mac.is_some() as u8,
            matches!(self.ip, IpAddr::V4(_)) as u8,
            iface,
        )
    }
}
