//! Typed wrappers for `ip -j neigh show`.

pub mod json;
#[cfg(all(unix, feature = "experimental-nl"))]
pub mod nl;

pub use crate::subcommands::Backend;
use crate::utils::serialize::mac::option_mac;
use std::net::IpAddr;

use macaddr::MacAddr;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Structured neighbor query input matching the common `ip neigh` flags.
#[derive(Debug, PartialEq, Eq, Clone, Hash, Serialize, Deserialize)]
pub struct NeighborInput {
    /// Destination address filter. The optional `to` keyword in CLI form is implicit here.
    pub to: Option<IpAddr>,
    /// Interface-name filter.
    pub dev: Option<String>,
    /// Neighbor-state filters.
    pub nud: Vec<NUDState>,
}

/// Linux neighbor reachability states.
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

    Other(u16),
}

/// One neighbor row from `ip -j neigh show`.
#[derive(Debug, PartialEq, Eq, Clone, Hash, Serialize, Deserialize)]
pub struct NeighborItem {
    #[serde(rename(deserialize = "dst"))]
    pub ip: IpAddr,
    #[serde(default)]
    pub dev: Option<String>,
    #[serde(with = "option_mac", default, rename(deserialize = "lladdr"))]
    pub mac: Option<MacAddr>,
    #[serde(default)]
    pub state: Vec<NUDState>,
}

/// Fetch neighbor rows using the default backend.
pub async fn get(
    ip: Option<IpAddr>,
    dev: Option<&str>,
    nud: &[NUDState],
) -> anyhow::Result<Vec<NeighborItem>> {
    get_with_backend(Backend::Json, ip, dev, nud).await
}

/// Fetch neighbor rows using an explicit backend.
pub async fn get_with_backend(
    backend: Backend,
    ip: Option<IpAddr>,
    dev: Option<&str>,
    nud: &[NUDState],
) -> anyhow::Result<Vec<NeighborItem>> {
    match backend {
        Backend::Json => json::get(ip, dev, nud).await,
        #[cfg(all(unix, feature = "experimental-nl"))]
        Backend::Netlink => {
            let ips: Vec<IpAddr> = ip.into_iter().collect();
            let devs: Vec<&str> = dev.into_iter().collect();
            nl::get(&ips, &devs, nud, &[]).await
        }
    }
}
