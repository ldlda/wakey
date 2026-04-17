//! Operator-facing service API: inventory, leases, interfaces, wake, and query resolution.
//! The `wakey` binary is Linux-only; use `wakey-agent` / `wakey-control-plane` for remote control.

pub mod service;
pub mod utils;

pub use service::{
    broadcast_wake_targets, get_interface_summaries, get_interface_summary, get_ips, get_leases,
    inventory, leases_without_state, merge_devices, resolve_devices, resolve_query,
    resolve_selector, resolve_wake_targets, wake_explicit, wake_from_query, wake_targets,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use wakey_core::{
        DhcpLease, NeighborEntry, NeighborState, Presence, Query, WakeStatus, WakeTarget,
    };

    #[tokio::test]
    async fn resolve_query_parses_ip() {
        let query = resolve_query("192.168.1.10").await.expect("resolve query");
        assert_eq!(
            query,
            vec![Query::Ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)))]
        );
    }

    #[tokio::test]
    async fn resolve_query_parses_mac() {
        let query = resolve_query("aa:bb:cc:dd:ee:ff")
            .await
            .expect("resolve query");
        assert_eq!(
            query,
            vec![Query::Mac("aa:bb:cc:dd:ee:ff".parse().expect("mac"))]
        );
    }

    #[tokio::test]
    async fn resolve_query_parses_nud() {
        let query = resolve_query("reachable").await.expect("resolve query");
        assert_eq!(query, vec![Query::NeighborState(NeighborState::Reachable)]);
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
        let leases = vec![wakey_core::DhcpLeaseWithState {
            lease_line: DhcpLease {
                expires_epoch: 1,
                ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
                mac: "aa:bb:cc:dd:ee:ff".parse().expect("mac"),
                name: Some("pc".into()),
            },
            nud_state: None,
        }];
        let devices = merge_devices(neighbors, leases, &wakey_core::InventoryQuery::default());
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
