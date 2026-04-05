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
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::net::{IpAddr, Ipv4Addr};

use crate::subcommands::link::OperState;

/// i dont include what i dont know about (almost all ts)
#[derive(Serialize, Debug, Deserialize)]
pub struct AddrOutput {
    pub ifindex: u32,
    pub ifname: String,
    /// i imagine UP or DOWN, unknown
    pub operstate: OperState,
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
    pub family: Option<AddressFamily>,

    #[serde(flatten, default)]
    pub cidr: InterfaceCidr,

    pub broadcast: Option<Ipv4Addr>,

    pub scope: Option<String>,
    pub label: Option<String>,
    // many more exist; we only take what we need
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressFamily {
    Inet,
    Inet6,
    Other,
}

impl AddressFamily {
    pub fn parse_lossy(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "inet" => Self::Inet,
            "inet6" => Self::Inet6,
            _ => Self::Other,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inet => "inet",
            Self::Inet6 => "inet6",
            Self::Other => "other",
        }
    }
}

impl Serialize for AddressFamily {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AddressFamily {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Self::parse_lossy(&value))
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct InterfaceCidr {
    pub local: Option<IpAddr>,
    pub prefixlen: Option<u8>,
}

impl InterfaceCidr {
    pub fn is_complete(&self) -> bool {
        self.local.is_some() && self.prefixlen.is_some()
    }
}

impl AddrInfo {
    pub fn local_addr(&self) -> Option<IpAddr> {
        self.cidr.local
    }

    pub fn prefixlen(&self) -> Option<u8> {
        self.cidr.prefixlen
    }

    pub fn is_ipv4(&self) -> bool {
        matches!(self.family, Some(AddressFamily::Inet))
    }

    pub fn is_ipv6(&self) -> bool {
        matches!(self.family, Some(AddressFamily::Inet6))
    }
}

impl AddrOutput {
    pub fn ipv4_addrs(&self) -> impl Iterator<Item = &AddrInfo> {
        self.addr_info.iter().filter(|info| info.is_ipv4())
    }

    pub fn ipv6_addrs(&self) -> impl Iterator<Item = &AddrInfo> {
        self.addr_info.iter().filter(|info| info.is_ipv6())
    }
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

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::{AddrInfo, AddrOutput, AddressFamily};
    use crate::subcommands::link::OperState;

    #[test]
    fn address_family_parses_known_values() {
        assert_eq!(AddressFamily::parse_lossy("inet"), AddressFamily::Inet);
        assert_eq!(AddressFamily::parse_lossy("INET6"), AddressFamily::Inet6);
        assert_eq!(AddressFamily::parse_lossy("weird"), AddressFamily::Other);
    }

    #[test]
    fn addr_info_deserializes_typed_ip_fields() {
        let info: AddrInfo = serde_json::from_str(
            r#"{"family":"inet","local":"192.168.1.1","prefixlen":24,"broadcast":"192.168.1.255","scope":"global"}"#,
        )
        .expect("addr_info json should deserialize");

        assert_eq!(info.family, Some(AddressFamily::Inet));
        assert_eq!(
            info.local_addr(),
            Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)))
        );
        assert_eq!(info.prefixlen(), Some(24));
        assert_eq!(info.broadcast, Some(Ipv4Addr::new(192, 168, 1, 255)));
        assert!(info.cidr.is_complete());
    }

    #[test]
    fn addr_output_deserializes_typed_operstate() {
        let output: AddrOutput = serde_json::from_str(
            r#"{"ifindex":2,"ifname":"br-lan","operstate":"UP","address":"aa:bb:cc:dd:ee:ff","addr_info":[]}"#,
        )
        .expect("addr_output json should deserialize");

        assert_eq!(output.operstate, OperState::Up);
    }
}
