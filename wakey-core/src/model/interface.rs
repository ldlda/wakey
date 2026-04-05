use macaddr::MacAddr;
use serde::Serialize;
use serde_with::skip_serializing_none;

use crate::parse::mac;

/// A condensed, operator-oriented view of one network interface.
///
/// This is intentionally smaller than the full Linux `ip address show` / `ip link show`
/// payload. It keeps the fields that are currently useful to `wakey`:
/// interface identity, operational state, MAC address, bound addresses, and
/// IPv4 broadcast targets.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize)]
pub struct InterfaceSummary {
    /// Kernel interface index.
    pub ifindex: u32,
    /// Interface name such as `br-lan`, `eth0`, or `wlan0`.
    pub ifname: String,
    /// Lowercased operational state such as `up`, `down`, or `unknown`.
    pub operstate: String,
    #[serde(with = "mac::option_mac")]
    /// Link-layer address when one exists.
    pub mac: Option<MacAddr>,
    /// Interface-bound addresses projected into a smaller usable shape.
    pub addrs: Vec<InterfaceAddr>,
}

/// A condensed view of one bound interface address.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize)]
pub struct InterfaceAddr {
    /// Address family such as `inet` or `inet6`.
    pub family: Option<String>,
    /// CIDR notation such as `192.168.1.1/24`.
    pub cidr: Option<String>,
    /// IPv4 broadcast target when Linux reports one.
    pub broadcast: Option<std::net::Ipv4Addr>,
    /// Linux-reported address scope, for example `global` or `link`.
    pub scope: Option<String>,
    /// Optional Linux label for the address entry.
    pub label: Option<String>,
}
