use anyhow::{Context, Result};
use macaddr::MacAddr;
use std::net::IpAddr;
use tracing::{debug, instrument};
use wakey_core::{InterfaceSummary, WakeResult, WakeTarget};

use crate::service::interfaces::get_interface_summaries;
use crate::service::inventory::InventorySources;
use crate::utils::product;

/// A wake request is either a device query or a MAC with an optional destination IP.
#[derive(Debug, PartialEq, Eq)]
pub enum WakeInput {
    Query(String),
    Explicit { mac: MacAddr, ip: Option<IpAddr> },
}

impl WakeInput {
    /// Validate the separate fields used by the CLI and agent protocol.
    pub fn from_parts(
        query: Option<String>,
        mac: Option<MacAddr>,
        ip: Option<IpAddr>,
    ) -> Result<Self> {
        match (query, mac, ip) {
            (_, None, Some(_)) => anyhow::bail!("wake IP requires a MAC address"),
            (Some(query), None, None) => Ok(Self::Query(query)),
            (None, Some(mac), ip) => Ok(Self::Explicit { mac, ip }),
            (Some(_), Some(_), _) => {
                anyhow::bail!("wake query and explicit MAC/IP are mutually exclusive")
            }
            (None, None, None) => anyhow::bail!("wake requires a query or a MAC address"),
        }
    }

    /// Resolve and send Wake-on-LAN packets for this input.
    pub async fn send(self) -> Result<WakeResult> {
        self.send_with_sources(&InventorySources::from_env()).await
    }

    /// Use the provided inventory for query mode. Explicit MAC/IP bypasses inventory.
    pub async fn send_with_sources(self, sources: &InventorySources) -> Result<WakeResult> {
        match self {
            Self::Query(query) => wake_from_query_with_sources(query, sources).await,
            Self::Explicit { mac, ip } => wake_explicit(mac, ip).await,
        }
    }
}

/// Send Wake-on-LAN packets for already-concrete wake targets.
#[instrument(skip_all, fields(targets = targets.len()))]
pub async fn wake_targets(targets: Vec<WakeTarget>) -> Result<WakeResult> {
    let result = wakey_linux::wake::wake_many(targets)
        .await
        .context("failed to send wake packets")?;
    debug!(results = result.len(), "wake packets sent");
    Ok(WakeResult { result })
}

/// Resolve free-form input into wake targets and send the packets.
pub async fn wake_from_query(input: impl Into<String>) -> Result<WakeResult> {
    wake_from_query_with_sources(input, &InventorySources::from_env()).await
}

#[instrument(name = "wake_from_query", skip_all)]
async fn wake_from_query_with_sources(
    input: impl Into<String>,
    sources: &InventorySources,
) -> Result<WakeResult> {
    let targets = resolve_wake_targets_with_sources(input, sources).await?;
    wake_targets(targets).await
}

/// Build broadcast wake targets for every broadcast-capable interface.
///
/// This is used by explicit manual wake mode when only a MAC address is supplied.
#[instrument(skip_all)]
pub async fn broadcast_wake_targets(mac: MacAddr) -> Result<Vec<WakeTarget>> {
    let interfaces = get_interface_summaries().await?;
    broadcast_wake_targets_from_interfaces(&interfaces, mac)
}

/// Wake a device explicitly by MAC, optionally targeting a specific IP/broadcast.
#[instrument(skip_all, fields(has_ip = ip.is_some()))]
pub async fn wake_explicit(mac: MacAddr, ip: Option<IpAddr>) -> Result<WakeResult> {
    let targets = match ip {
        Some(ip) => explicit_wake_targets_for_ip(mac, ip),
        None => broadcast_wake_targets(mac).await?,
    };
    wake_targets(targets).await
}

/// Resolve free-form input into concrete wake targets.
///
/// The current resolution strategy pairs each device IP with each of its MACs.
pub async fn resolve_wake_targets(input: impl Into<String>) -> Result<Vec<WakeTarget>> {
    resolve_wake_targets_with_sources(input, &InventorySources::from_env()).await
}

#[instrument(name = "resolve_wake_targets", skip_all)]
async fn resolve_wake_targets_with_sources(
    input: impl Into<String>,
    sources: &InventorySources,
) -> Result<Vec<WakeTarget>> {
    let devices = sources.resolve_devices(input).await?;

    let targets: Vec<WakeTarget> = devices
        .into_iter()
        .flat_map(|device| {
            // TRY EVERYTHING some has to work
            product(device.ips, device.macs).map(|(i, m)| WakeTarget {
                ip: Some(i),
                mac: Some(m),
            })
        })
        .collect();
    debug!(targets = targets.len(), "resolved wake targets");
    Ok(targets)
}

fn explicit_wake_targets_for_ip(mac: MacAddr, ip: IpAddr) -> Vec<WakeTarget> {
    vec![WakeTarget {
        ip: Some(ip),
        mac: Some(mac),
    }]
}

fn broadcast_wake_targets_from_interfaces(
    interfaces: &[InterfaceSummary],
    mac: MacAddr,
) -> Result<Vec<WakeTarget>> {
    let targets: Vec<WakeTarget> = interfaces
        .iter()
        .flat_map(|iface| iface.addrs.iter())
        .filter_map(|addr| addr.broadcast)
        .map(|br| WakeTarget {
            ip: Some(IpAddr::V4(br)), // some will hit eventually AHHH logic
            mac: Some(mac),
        })
        .collect();

    if targets.is_empty() {
        anyhow::bail!("no broadcast-capable interfaces found");
    }

    debug!(
        targets = targets.len(),
        interfaces = interfaces.len(),
        "built broadcast wake targets"
    );
    Ok(targets)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::{WakeInput, broadcast_wake_targets_from_interfaces, explicit_wake_targets_for_ip};
    use wakey_core::{InterfaceAddr, InterfaceSummary};

    #[test]
    fn wake_input_accepts_query_or_explicit_mac() {
        let mac = "aa:bb:cc:dd:ee:ff".parse().unwrap();
        let ip = "192.168.1.10".parse().unwrap();
        assert_eq!(
            WakeInput::from_parts(Some("pc".into()), None, None).unwrap(),
            WakeInput::Query("pc".into())
        );
        for ip in [None, Some(ip)] {
            assert_eq!(
                WakeInput::from_parts(None, Some(mac), ip).unwrap(),
                WakeInput::Explicit { mac, ip }
            );
        }
    }

    #[test]
    fn wake_input_rejects_missing_and_mixed_addressing() {
        let mac = "aa:bb:cc:dd:ee:ff".parse().unwrap();
        let ip = "192.168.1.10".parse().unwrap();
        for (query, mac, ip, expected) in [
            (None, None, None, "requires a query or a MAC"),
            (None, None, Some(ip), "IP requires a MAC"),
            (Some("pc"), None, Some(ip), "IP requires a MAC"),
            (Some("pc"), Some(mac), None, "mutually exclusive"),
            (Some("pc"), Some(mac), Some(ip), "mutually exclusive"),
        ] {
            let err = WakeInput::from_parts(query.map(str::to_owned), mac, ip).unwrap_err();
            assert!(err.to_string().contains(expected), "{err}");
        }
    }

    #[test]
    fn explicit_wake_target_for_ip_builds_one_complete_target() {
        let mac: macaddr::MacAddr = "aa:bb:cc:dd:ee:ff".parse().expect("mac");
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 255));

        let targets = explicit_wake_targets_for_ip(mac, ip);

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].ip, Some(ip));
        assert_eq!(targets[0].mac, Some(mac));
    }

    #[test]
    fn broadcast_wake_targets_from_interfaces_errors_when_no_broadcast_exists() {
        let mac: macaddr::MacAddr = "aa:bb:cc:dd:ee:ff".parse().expect("mac");
        let interfaces = vec![InterfaceSummary {
            ifindex: 1,
            ifname: "eth0".into(),
            operstate: "up".into(),
            mac: None,
            addrs: vec![InterfaceAddr {
                family: Some("inet".into()),
                cidr: Some("192.168.1.10/24".into()),
                broadcast: None,
                scope: Some("global".into()),
                label: None,
            }],
        }];

        let err = broadcast_wake_targets_from_interfaces(&interfaces, mac)
            .expect_err("should error without broadcast-capable interfaces");

        assert!(
            err.to_string()
                .contains("no broadcast-capable interfaces found")
        );
    }

    #[test]
    fn broadcast_wake_targets_from_interfaces_builds_targets_from_broadcast_rows() {
        let mac: macaddr::MacAddr = "aa:bb:cc:dd:ee:ff".parse().expect("mac");
        let interfaces = vec![InterfaceSummary {
            ifindex: 2,
            ifname: "br-lan".into(),
            operstate: "up".into(),
            mac: None,
            addrs: vec![
                InterfaceAddr {
                    family: Some("inet".into()),
                    cidr: Some("192.168.1.1/24".into()),
                    broadcast: Some(Ipv4Addr::new(192, 168, 1, 255)),
                    scope: Some("global".into()),
                    label: None,
                },
                InterfaceAddr {
                    family: Some("inet6".into()),
                    cidr: Some("fe80::1/64".into()),
                    broadcast: None,
                    scope: Some("link".into()),
                    label: None,
                },
            ],
        }];

        let targets = broadcast_wake_targets_from_interfaces(&interfaces, mac)
            .expect("broadcast target resolution should succeed");

        assert_eq!(targets.len(), 1);
        assert_eq!(
            targets[0].ip,
            Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 255)))
        );
        assert_eq!(targets[0].mac, Some(mac));
    }
}
