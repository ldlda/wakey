//! Typed wrappers for `ip -j address show`.
//!
//! This module is intentionally close to the Linux output shape while still
//! tightening a few fields into more useful Rust types.

pub mod json;
#[cfg(all(unix, feature = "experimental-nl"))]
pub mod nl;

pub use crate::subcommands::Backend;
use crate::utils::serialize::mac::option_mac;
use macaddr::MacAddr;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr};

use crate::subcommands::link::OperState;

/// One interface row from `ip -j address show`.
#[derive(Serialize, Debug, Deserialize)]
pub struct AddrOutput {
    pub ifindex: u32,
    pub ifname: String,
    /// Interface operational state.
    pub operstate: OperState,
    #[serde(with = "option_mac", default)]
    pub address: Option<MacAddr>,
    /// Per-address entries attached to this interface.
    #[serde(default)]
    pub addr_info: Vec<AddrInfo>,
}

/// One address entry nested under an interface row.
#[derive(Debug, Deserialize, Serialize)]
pub struct AddrInfo {
    pub family: Option<AddressFamily>,

    #[serde(flatten, default)]
    pub cidr: InterfaceCidr,

    pub broadcast: Option<Ipv4Addr>,

    pub scope: Option<String>,
    pub label: Option<String>,
}

/// Address family used by `ip address` output.
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

/// Raw parsed local address plus prefix length.
///
/// This is still a source-shaped type; callers that need a guaranteed usable
/// CIDR should validate that both fields are present.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct InterfaceCidr {
    pub local: Option<IpAddr>,
    pub prefixlen: Option<u8>,
}

impl InterfaceCidr {
    /// Return whether both parts needed for a usable CIDR are present.
    pub fn is_complete(&self) -> bool {
        self.local.is_some() && self.prefixlen.is_some()
    }

    /// Return a validated complete CIDR when both fields are present.
    pub fn complete(&self) -> Option<CompleteInterfaceCidr> {
        Some(CompleteInterfaceCidr {
            local: self.local?,
            prefixlen: self.prefixlen?,
        })
    }

    /// Format the CIDR as `addr/prefixlen` when both fields are present.
    pub fn to_cidr_string(&self) -> Option<String> {
        self.complete().map(|cidr| cidr.to_string())
    }
}

/// Validated interface CIDR with both local address and prefix length present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompleteInterfaceCidr {
    pub local: IpAddr,
    pub prefixlen: u8,
}

impl fmt::Display for CompleteInterfaceCidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.local, self.prefixlen)
    }
}

impl AddrInfo {
    /// Return the parsed local IP address when present.
    pub fn local_addr(&self) -> Option<IpAddr> {
        self.cidr.local
    }

    /// Return the parsed prefix length when present.
    pub fn prefixlen(&self) -> Option<u8> {
        self.cidr.prefixlen
    }

    /// Return whether this row is IPv4.
    pub fn is_ipv4(&self) -> bool {
        matches!(self.family, Some(AddressFamily::Inet))
    }

    /// Return whether this row is IPv6.
    pub fn is_ipv6(&self) -> bool {
        matches!(self.family, Some(AddressFamily::Inet6))
    }
}

impl AddrOutput {
    /// Iterate IPv4 address entries.
    pub fn ipv4_addrs(&self) -> impl Iterator<Item = &AddrInfo> {
        self.addr_info.iter().filter(|info| info.is_ipv4())
    }

    /// Iterate IPv6 address entries.
    pub fn ipv6_addrs(&self) -> impl Iterator<Item = &AddrInfo> {
        self.addr_info.iter().filter(|info| info.is_ipv6())
    }
}

/// Fetch address data using the default backend.
pub async fn get(dev: Option<&str>) -> anyhow::Result<Vec<AddrOutput>> {
    get_with_backend(Backend::Json, dev).await
}

/// Fetch address data using an explicit backend.
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

    use super::{AddrInfo, AddrOutput, AddressFamily, InterfaceCidr};
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
        assert_eq!(
            info.cidr.to_cidr_string().as_deref(),
            Some("192.168.1.1/24")
        );
    }

    #[test]
    fn addr_output_deserializes_typed_operstate() {
        let output: AddrOutput = serde_json::from_str(
            r#"{"ifindex":2,"ifname":"br-lan","operstate":"UP","address":"aa:bb:cc:dd:ee:ff","addr_info":[]}"#,
        )
        .expect("addr_output json should deserialize");

        assert_eq!(output.operstate, OperState::Up);
    }

    #[test]
    fn interface_cidr_complete_formats() {
        let cidr = InterfaceCidr {
            local: Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))),
            prefixlen: Some(16),
        };

        let complete = cidr.complete().expect("cidr should be complete");
        assert_eq!(complete.to_string(), "10.0.0.1/16");
    }
}
