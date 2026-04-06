use anyhow::{Context, Result};
use macaddr::MacAddr;
use std::net::IpAddr;
use wakey_core::{InterfaceSummary, WakeResult, WakeTarget};

use crate::service::interfaces::get_interface_summaries;
use crate::service::inventory::resolve_devices;

/// Send Wake-on-LAN packets for already-concrete wake targets.
pub async fn wake_targets(targets: Vec<WakeTarget>) -> Result<WakeResult> {
    let result = wakey_linux::wake::wake_many(targets)
        .await
        .context("failed to send wake packets")?;
    Ok(WakeResult { result })
}

/// Resolve free-form input into wake targets and send the packets.
pub async fn wake_from_query(input: impl Into<String>) -> Result<WakeResult> {
    let targets = resolve_wake_targets(input).await?;
    wake_targets(targets).await
}

/// Build broadcast wake targets for every broadcast-capable interface.
///
/// This is used by explicit manual wake mode when only a MAC address is supplied.
pub async fn broadcast_wake_targets(mac: MacAddr) -> Result<Vec<WakeTarget>> {
    let interfaces = get_interface_summaries().await?;
    broadcast_wake_targets_from_interfaces(&interfaces, mac)
}

/// Wake a device explicitly by MAC, optionally targeting a specific IP/broadcast.
pub async fn wake_explicit(mac: MacAddr, ip: Option<IpAddr>) -> Result<WakeResult> {
    let targets = match ip {
        Some(ip) => explicit_wake_targets_for_ip(mac, ip),
        None => broadcast_wake_targets(mac).await?,
    };
    wake_targets(targets).await
}

/// Resolve free-form input into concrete wake targets.
///
/// The current resolution strategy fans out one wake target per resolved device IP,
/// using the first known MAC address for that device.
pub async fn resolve_wake_targets(input: impl Into<String>) -> Result<Vec<WakeTarget>> {
    let devices = resolve_devices(input).await?;
    Ok(devices
        .into_iter()
        .flat_map(|device| {
            let mac = device.macs.first().copied();
            device
                .ips
                .into_iter()
                .map(move |ip| WakeTarget { ip: Some(ip), mac })
        })
        .collect())
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
        .map(|ip| WakeTarget {
            ip: Some(IpAddr::V4(ip)),
            mac: Some(mac),
        })
        .collect();

    if targets.is_empty() {
        anyhow::bail!("no broadcast-capable interfaces found");
    }

    Ok(targets)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::{broadcast_wake_targets_from_interfaces, explicit_wake_targets_for_ip};
    use wakey_core::{InterfaceAddr, InterfaceSummary};

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
