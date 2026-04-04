//! ts
//!
//! deals with both ip a (addroutput) and ip l (commonoutput)
//!
//! lowk why its free but its indirection and its ass

pub mod json;
#[cfg(all(unix, feature = "experimental-nl"))]
pub mod nl;

pub use crate::subcommands::Backend;
use crate::utils::serialize::mac::option_mac;
use macaddr::MacAddr;
use serde::{Deserialize, Serialize};

/// i dont include what i dont know about (almost all ts)
#[derive(Serialize, Debug, Deserialize)]
pub struct AddrOutput {
    pub ifindex: u32,
    pub ifname: String,
    /// i imagine UP or DOWN, unknown
    pub operstate: String,
    // 6 has a serde and the enum doesnt? why. (serializing ts is ass although... im not given an array. they string formatted ts)
    #[serde(with = "option_mac", default)]
    pub address: Option<MacAddr>,
    #[serde(default)] // i wish we have intellisense for this... fuck you metaprogramming
    pub addr_info: Vec<AddrInfo>,
}

// i be copying
// Raw JSON shape from ip -j -4 address show
#[derive(Debug, Deserialize, Serialize)]
pub struct AddrInfo {
    pub family: Option<String>,

    pub local: Option<String>,
    pub prefixlen: Option<u8>,

    pub broadcast: Option<String>,

    pub scope: Option<String>,
    pub label: Option<String>,
    // many more exist; we only take what we need
}

pub async fn get(dev: Option<&str>) -> anyhow::Result<Vec<AddrOutput>> {
    get_with_backend(Backend::Json, dev).await
}

pub async fn get_with_backend(
    backend: Backend,
    dev: Option<&str>,
) -> anyhow::Result<Vec<AddrOutput>> {
    match backend {
        Backend::Json => json::get(dev).await,
        #[cfg(all(unix, feature = "experimental-nl"))]
        Backend::Netlink => nl::get(dev).await,
    }
}
