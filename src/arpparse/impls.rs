//! r#impl AHHHHHH

use serde::{Deserialize, Deserializer, de};

use super::NUDState;

// Case-insensitive parsing for NUDState via manual Deserialize
impl<'de> Deserialize<'de> for NUDState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: &str = <&str as Deserialize>::deserialize(deserializer)?;
        s.parse().map_err(de::Error::custom)
    }
}

use crate::arpparse::IpNeighLine;

use lda_ipjs::subcommands::neighbor::{self as ipjs_neigh, NeighborItem};

impl From<ipjs_neigh::NUDState> for NUDState {
    fn from(value: ipjs_neigh::NUDState) -> Self {
        match value {
            ipjs_neigh::NUDState::Permanent => NUDState::Permanent,
            ipjs_neigh::NUDState::Noarp => NUDState::Noarp,
            ipjs_neigh::NUDState::Reachable => NUDState::Reachable,
            ipjs_neigh::NUDState::Stale => NUDState::Stale,
            ipjs_neigh::NUDState::None => NUDState::None,
            ipjs_neigh::NUDState::Incomplete => NUDState::Incomplete,
            ipjs_neigh::NUDState::Delay => NUDState::Delay,
            ipjs_neigh::NUDState::Probe => NUDState::Probe,
            ipjs_neigh::NUDState::Failed => NUDState::Failed,
            ipjs_neigh::NUDState::Other(_) => NUDState::None,
        }
    }
}

impl From<NUDState> for ipjs_neigh::NUDState {
    fn from(value: NUDState) -> Self {
        match value {
            NUDState::Permanent => ipjs_neigh::NUDState::Permanent,
            NUDState::Noarp => ipjs_neigh::NUDState::Noarp,
            NUDState::Reachable => ipjs_neigh::NUDState::Reachable,
            NUDState::Stale => ipjs_neigh::NUDState::Stale,
            NUDState::None => ipjs_neigh::NUDState::None,
            NUDState::Incomplete => ipjs_neigh::NUDState::Incomplete,
            NUDState::Delay => ipjs_neigh::NUDState::Delay,
            NUDState::Probe => ipjs_neigh::NUDState::Probe,
            NUDState::Failed => ipjs_neigh::NUDState::Failed,
        }
    }
}

impl From<NeighborItem> for IpNeighLine {
    fn from(item: NeighborItem) -> Self {
        IpNeighLine {
            ip: item.ip,
            dev: Some(item.dev),
            mac: item.mac,
            state: item
                .state
                .first()
                .copied()
                .map(Into::into)
                .unwrap_or(NUDState::None),
        }
    }
}
