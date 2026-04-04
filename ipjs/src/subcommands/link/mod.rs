//! Typed wrappers for `ip -j link show`.

pub mod json;
#[cfg(all(unix, feature = "experimental-nl"))]
pub mod nl;

pub use crate::subcommands::Backend;
use crate::utils::serialize::mac::option_mac;
use macaddr::MacAddr;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkOutput {
    pub ifindex: u32,
    pub ifname: String,
    #[serde(default)]
    pub operstate: Option<String>,
    #[serde(default, with = "option_mac")]
    pub address: Option<MacAddr>,
}

pub async fn get(dev: Option<&str>) -> anyhow::Result<Vec<LinkOutput>> {
    get_with_backend(Backend::Json, dev).await
}

pub async fn get_with_backend(
    backend: Backend,
    dev: Option<&str>,
) -> anyhow::Result<Vec<LinkOutput>> {
    match backend {
        Backend::Json => json::get(dev).await,
        #[cfg(all(unix, feature = "experimental-nl"))]
        Backend::Netlink => nl::get(dev).await,
    }
}
