pub mod arpparse;
pub mod dhcpparse;
pub mod route;
pub mod utils;

use std::{io, net::SocketAddr};

use anyhow::{Context, Result};
use axum::Router;
use tokio::net::TcpListener;
use tower_http::services::ServeDir;
use wakey_core::{
    DeviceFilters, DeviceQuery, DhcpLease, DhcpLeaseWithState, NeighborEntry, QueryInput, Status,
    WakeResult, WakeTarget,
};

pub type StatusResponse = Status<NeighborEntry>;

pub async fn resolve_query(input: impl Into<String>) -> Result<DeviceQuery> {
    Ok(match wakey_linux::devices::classify_query(input.into()).await {
        QueryInput::Ip(ip_addr) => DeviceQuery {
            filter: DeviceFilters {
                ips: vec![ip_addr],
                ..Default::default()
            },
            ..Default::default()
        },
        QueryInput::Mac(mac_addr) => DeviceQuery {
            filter: DeviceFilters {
                macs: vec![mac_addr],
                ..Default::default()
            },
            ..Default::default()
        },
        QueryInput::Dev(dev) => DeviceQuery {
            filter: DeviceFilters {
                devs: vec![dev],
                ..Default::default()
            },
            ..Default::default()
        },
        QueryInput::Nud(state) => DeviceQuery {
            filter: DeviceFilters {
                nuds: vec![state],
                ..Default::default()
            },
            ..Default::default()
        },
        QueryInput::Name(name) => DeviceQuery {
            name: Some(name),
            ..Default::default()
        },
    })
}

pub async fn get_status(query: DeviceQuery) -> Result<StatusResponse> {
    let table = wakey_linux::devices::query_status(&query).await?;
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

pub async fn get_leases(include_state: bool) -> Result<Vec<DhcpLeaseWithState>> {
    let leases = wakey_linux::dhcp::read_dhcp_leases_with_names()
        .await
        .context("failed to read DHCP leases")?;
    if include_state {
        Ok(wakey_linux::dhcp::enrich_leases_with_nud_state(leases).await)
    } else {
        Ok(leases
            .into_iter()
            .map(|lease_line| DhcpLeaseWithState {
                lease_line,
                nud_state: None,
            })
            .collect())
    }
}

pub async fn wake_targets(targets: Vec<WakeTarget>) -> Result<WakeResult> {
    let result = wakey_linux::wake::wake_many(targets)
        .await
        .context("failed to send wake packets")?;
    Ok(WakeResult { result })
}

pub async fn wake_from_query(input: impl Into<String>) -> Result<WakeResult> {
    let status = get_status_for_input(input).await?;
    let targets = status
        .table
        .into_iter()
        .map(|entry| WakeTarget {
            ip: Some(entry.ip),
            mac: entry.mac,
        })
        .collect();
    wake_targets(targets).await
}

pub async fn list_interfaces() -> Result<Vec<String>> {
    Ok(wakey_linux::devices::devs_sorted().await)
}

pub async fn get_ips(name: impl AsRef<str>) -> Result<Vec<std::net::IpAddr>> {
    Ok(wakey_linux::devices::get_ips(name.as_ref())
        .await?
        .collect())
}

pub fn http_app(static_root: std::path::PathBuf) -> Router {
    Router::new()
        .nest("/api", route::api_router())
        .fallback_service(
            axum::routing::get_service(
                ServeDir::new(static_root)
                    .append_index_html_on_directories(true)
                    .precompressed_br()
                    .precompressed_deflate()
                    .precompressed_gzip()
                    .precompressed_zstd(),
            ),
        )
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
        assert_eq!(query.filter.ips, vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10))]);
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
