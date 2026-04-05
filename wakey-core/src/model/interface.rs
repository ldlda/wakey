use macaddr::MacAddr;
use serde::Serialize;
use serde_with::skip_serializing_none;

use crate::parse::mac;

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
