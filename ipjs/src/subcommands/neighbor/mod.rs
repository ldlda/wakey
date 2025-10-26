//! ```bash
//! ip -j n s
//! ```
//!
//! yes. this is a real call.

pub mod json;
pub mod nl;

use crate::utils::serialize::mac::{des_opm, ser_opm};
use std::net::IpAddr;

use macaddr::MacAddr;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

#[derive(Debug, PartialEq, Eq, Clone, Hash, Serialize, Deserialize)]
pub struct NeighborInput {
    /// supports only the last item (ignore), `to` keyword is optional
    to: Option<IpAddr>,
    /// supports only one item (it complains if multiple)
    dev: Option<String>, // im all for simplicity
    /// takes multiple, has to have `nud` before bro or it will think you `to`
    nud: Vec<NUDState>,
}

// as input this must be lowercase. as output it is uppercase
/// docs for items come from a random ahh man website idk
#[derive(
    Debug, PartialEq, Eq, EnumString, Display, Clone, Copy, Hash, Serialize, Deserialize, Default,
)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
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
    #[default]
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

/// everything i see
#[derive(Debug, PartialEq, Eq, Clone, Hash, Serialize, Deserialize)]
pub struct NeighborItem {
    dst: IpAddr,
    dev: String,
    #[serde(deserialize_with = "des_opm", serialize_with = "ser_opm")]
    lladdr: Option<MacAddr>,
    state: Vec<NUDState>,
}
