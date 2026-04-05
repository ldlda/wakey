pub mod arpparse;
pub mod compat;
pub mod dhcpparse;
pub mod route;
pub mod utils;

use std::{io, net::SocketAddr};

use anyhow::{Context, Result};
use axum::Router;
use tokio::net::TcpListener;
use tower_http::services::ServeDir;
use wakey_core::{
    Device, DeviceFilters, DeviceInventory, DeviceQuery, DhcpLease, DhcpLeaseWithState,
    InterfaceSummary, LeaseQuery, NeighborEntry, Presence, Query, QueryInput, Status, WakeResult,
    WakeTarget,
};

pub type StatusResponse = Status<NeighborEntry>;

pub async fn resolve_query(input: impl Into<String>) -> Result<DeviceQuery> {
    query_to_device_query(resolve_selector(input).await?)
}

pub async fn resolve_selector(input: impl Into<String>) -> Result<Query> {
    Ok(
        match wakey_linux::devices::classify_query(input.into()).await {
            QueryInput::Ip(ip_addr) => Query::Ip(ip_addr),
            QueryInput::Mac(mac_addr) => Query::Mac(mac_addr),
            QueryInput::Dev(dev) => Query::Interface(dev),
            QueryInput::Nud(state) => Query::NeighborState(state),
            QueryInput::Name(name) => Query::Text(name),
        },
    )
}

pub fn query_to_device_query(query: Query) -> Result<DeviceQuery> {
    Ok(match query {
        Query::Ip(ip_addr) => DeviceQuery {
            filter: DeviceFilters {
                ips: vec![ip_addr],
                ..Default::default()
            },
            ..Default::default()
        },
        Query::Mac(mac_addr) => DeviceQuery {
            filter: DeviceFilters {
                macs: vec![mac_addr],
                ..Default::default()
            },
            ..Default::default()
        },
        Query::Interface(dev) => DeviceQuery {
            filter: DeviceFilters {
                devs: vec![dev],
                ..Default::default()
            },
            ..Default::default()
        },
        Query::NeighborState(state) => DeviceQuery {
            filter: DeviceFilters {
                nuds: vec![state],
                ..Default::default()
            },
            ..Default::default()
        },
        Query::Text(name) => DeviceQuery {
            name: Some(name),
            ..Default::default()
        },
    })
}

pub async fn get_status(query: DeviceQuery) -> Result<StatusResponse> {
    let inventory = inventory(query.clone()).await?;
    let table = inventory
        .devices
        .iter()
        .flat_map(device_to_status_rows)
        .collect();
    Ok(Status {
        name: query.name,
        table,
        filters: query.filter,
    })
}

pub async fn get_status_for_input(input: impl Into<String>) -> Result<StatusResponse> {
    let query = resolve_query(input).await?;
    get_status(query).await
}

pub async fn get_leases(query: LeaseQuery) -> Result<Vec<DhcpLeaseWithState>> {
    let leases = wakey_linux::dhcp::read_dhcp_leases_with_names()
        .await
        .context("failed to read DHCP leases")?;
    if query.include_state {
        Ok(wakey_linux::dhcp::enrich_leases_with_nud_state(leases).await)
    } else {
        Ok(leases_without_state(leases))
    }
}

pub async fn wake_targets(targets: Vec<WakeTarget>) -> Result<WakeResult> {
    let result = wakey_linux::wake::wake_many(targets)
        .await
        .context("failed to send wake packets")?;
    Ok(WakeResult { result })
}

pub async fn wake_from_query(input: impl Into<String>) -> Result<WakeResult> {
    let targets = resolve_wake_targets(input).await?;
    wake_targets(targets).await
}

pub async fn broadcast_wake_targets(mac: macaddr::MacAddr) -> Result<Vec<WakeTarget>> {
    Ok(get_interface_summaries()
        .await?
        .into_iter()
        .flat_map(|iface| iface.addrs.into_iter())
        .filter_map(|addr| addr.broadcast)
        .map(|ip| WakeTarget {
            ip: Some(std::net::IpAddr::V4(ip)),
            mac: Some(mac),
        })
        .collect())
}

pub async fn wake_explicit(mac: macaddr::MacAddr, ip: Option<std::net::IpAddr>) -> Result<WakeResult> {
    let targets = match ip {
        Some(ip) => vec![WakeTarget {
            ip: Some(ip),
            mac: Some(mac),
        }],
        None => broadcast_wake_targets(mac).await?,
    };
    wake_targets(targets).await
}

pub async fn list_interfaces() -> Result<Vec<String>> {
    Ok(wakey_linux::devices::devs_sorted().await)
}

pub async fn get_interface_summaries() -> Result<Vec<InterfaceSummary>> {
    wakey_linux::devices::list_interface_summaries().await
}

pub async fn get_interface_summary(name: &str) -> Result<Option<InterfaceSummary>> {
    Ok(get_interface_summaries()
        .await?
        .into_iter()
        .find(|iface| iface.ifname == name))
}

pub async fn get_ips(name: impl AsRef<str>) -> Result<Vec<std::net::IpAddr>> {
    Ok(wakey_linux::devices::get_ips(name.as_ref())
        .await?
        .collect())
}

pub async fn resolve_devices(input: impl Into<String>) -> Result<Vec<Device>> {
    let query = resolve_query(input).await?;
    inventory(query).await.map(|inventory| inventory.devices)
}

pub async fn inventory(query: DeviceQuery) -> Result<DeviceInventory> {
    let neighbors = wakey_linux::devices::query_status(&query).await?;
    let leases = get_leases(LeaseQuery {
        include_state: false,
    })
    .await?;
    Ok(DeviceInventory {
        devices: merge_devices(neighbors, leases, &query),
    })
}

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

fn merge_devices(
    neighbors: Vec<NeighborEntry>,
    leases: Vec<DhcpLeaseWithState>,
    query: &DeviceQuery,
) -> Vec<Device> {
    use std::collections::BTreeMap;

    let mut by_mac: BTreeMap<String, (Vec<NeighborEntry>, Vec<DhcpLease>)> = BTreeMap::new();

    for row in neighbors {
        let key = row
            .mac
            .map(|m| m.to_string())
            .unwrap_or_else(|| format!("ip:{}", row.ip));
        by_mac.entry(key).or_default().0.push(row);
    }
    for lease in leases {
        let key = lease.lease_line.mac.to_string();
        by_mac.entry(key).or_default().1.push(lease.lease_line);
    }

    let mut devices: Vec<Device> = by_mac
        .into_values()
        .map(|(neighbors, leases)| Device::from_parts(neighbors, leases))
        .collect();

    if let Some(name) = &query.name {
        devices.retain(|device| device.names.iter().any(|n| n == name));
    }
    if !query.filter.devs.is_empty() {
        devices.retain(|device| {
            device
                .interfaces
                .iter()
                .any(|iface| query.filter.devs.contains(iface))
        });
    }
    if !query.filter.ips.is_empty() {
        devices.retain(|device| device.ips.iter().any(|ip| query.filter.ips.contains(ip)));
    }
    if !query.filter.macs.is_empty() {
        devices.retain(|device| {
            device
                .macs
                .iter()
                .any(|mac| query.filter.macs.contains(mac))
        });
    }
    if !query.filter.nuds.is_empty() {
        devices.retain(|device| {
            device
                .neighbors
                .iter()
                .any(|neighbor| query.filter.nuds.contains(&neighbor.state))
        });
    }

    devices.sort_by(|a, b| {
        presence_rank(b.presence)
            .cmp(&presence_rank(a.presence))
            .then_with(|| a.names.first().cmp(&b.names.first()))
    });
    devices
}

const fn presence_rank(presence: Presence) -> u8 {
    match presence {
        Presence::Online => 3,
        Presence::LikelyOnline => 2,
        Presence::Unknown => 1,
        Presence::Offline => 0,
    }
}

fn device_to_status_rows(device: &Device) -> Vec<NeighborEntry> {
    if !device.neighbors.is_empty() {
        return device.neighbors.clone();
    }

    let fallback_mac = device.macs.first().copied();
    let fallback_dev = device.interfaces.first().cloned();
    let fallback_state = match device.presence {
        Presence::Online => wakey_core::NeighborState::Reachable,
        Presence::LikelyOnline => wakey_core::NeighborState::Stale,
        Presence::Offline => wakey_core::NeighborState::Failed,
        Presence::Unknown => wakey_core::NeighborState::None,
    };

    device
        .ips
        .iter()
        .copied()
        .map(|ip| NeighborEntry {
            ip,
            dev: fallback_dev.clone(),
            mac: fallback_mac,
            state: fallback_state,
        })
        .collect()
}

pub fn http_app(static_root: std::path::PathBuf) -> Router {
    Router::new()
        .nest("/api", route::api_router())
        .fallback_service(axum::routing::get_service(
            ServeDir::new(static_root)
                .append_index_html_on_directories(true)
                .precompressed_br()
                .precompressed_deflate()
                .precompressed_gzip()
                .precompressed_zstd(),
        ))
}

pub async fn serve_http(addr: SocketAddr, static_root: std::path::PathBuf) -> io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, http_app(static_root).into_make_service()).await
}

pub async fn serve_http_from_current_exe(addr: SocketAddr) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let root = exe
        .parent()
        .ok_or_else(|| io::Error::other("no parent dir"))?;
    serve_http(addr, root.join("static")).await
}

pub fn leases_without_state(leases: Vec<DhcpLease>) -> Vec<DhcpLeaseWithState> {
    leases
        .into_iter()
        .map(|lease_line| DhcpLeaseWithState {
            lease_line,
            nud_state: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use wakey_core::{NeighborState, WakeStatus};

    #[tokio::test]
    async fn resolve_query_parses_ip() {
        let query = resolve_query("192.168.1.10").await.expect("resolve query");
        assert_eq!(
            query.filter.ips,
            vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10))]
        );
    }

    #[tokio::test]
    async fn resolve_query_parses_mac() {
        let query = resolve_query("aa:bb:cc:dd:ee:ff")
            .await
            .expect("resolve query");
        assert_eq!(query.filter.macs.len(), 1);
    }

    #[tokio::test]
    async fn resolve_query_parses_nud() {
        let query = resolve_query("reachable").await.expect("resolve query");
        assert_eq!(query.filter.nuds, vec![NeighborState::Reachable]);
    }

    #[tokio::test]
    async fn resolve_selector_keeps_text_vs_structured() {
        let selector = resolve_selector("reachable")
            .await
            .expect("resolve selector");
        match selector {
            Query::NeighborState(NeighborState::Reachable) => {}
            _ => panic!("expected neighbor-state selector"),
        }
    }

    #[test]
    fn leases_without_state_clears_nud_state() {
        let leases = vec![DhcpLease {
            expires_epoch: 1,
            ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            mac: "aa:bb:cc:dd:ee:ff".parse().expect("mac"),
            name: Some("pc".into()),
        }];
        let out = leases_without_state(leases);
        assert_eq!(out.len(), 1);
        assert!(out[0].nud_state.is_none());
    }

    #[test]
    fn merge_devices_combines_lease_and_neighbor() {
        let neighbors = vec![NeighborEntry {
            ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
            dev: Some("br-lan".into()),
            mac: Some("aa:bb:cc:dd:ee:ff".parse().expect("mac")),
            state: NeighborState::Reachable,
        }];
        let leases = vec![DhcpLeaseWithState {
            lease_line: DhcpLease {
                expires_epoch: 1,
                ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
                mac: "aa:bb:cc:dd:ee:ff".parse().expect("mac"),
                name: Some("pc".into()),
            },
            nud_state: None,
        }];
        let devices = merge_devices(neighbors, leases, &DeviceQuery::default());
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].presence, Presence::Online);
        assert_eq!(devices[0].names, vec!["pc".to_string()]);
    }

    #[tokio::test]
    async fn wake_targets_marks_incomplete() {
        let out = wake_targets(vec![WakeTarget {
            ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            mac: None,
        }])
        .await
        .expect("wake");
        assert_eq!(out.result.len(), 1);
        assert_eq!(out.result[0].status, WakeStatus::Incomplete);
    }
}
