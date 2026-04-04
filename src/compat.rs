use serde::Serialize;
use wakey_core::parse::mac;
use wakey_core::{
    DeviceFilters, DhcpLeaseWithState, NeighborEntry, Status, WakeResult, WakeTargetResult,
};

#[derive(Debug, Clone, Serialize)]
pub struct LegacyStatusRow {
    pub ip: std::net::IpAddr,
    pub dev: Option<String>,
    #[serde(with = "mac::option_mac")]
    pub mac: Option<macaddr::MacAddr>,
    pub state: wakey_core::NeighborState,
}

#[derive(Debug, Clone, Serialize)]
pub struct LegacyStatusResponse {
    pub name: Option<String>,
    pub table: Vec<LegacyStatusRow>,
    pub filters: DeviceFilters,
}

#[derive(Debug, Clone, Serialize)]
pub struct LegacyLeaseRow {
    pub expires_epoch: u64,
    pub ip: std::net::IpAddr,
    #[serde(with = "mac")]
    pub mac: macaddr::MacAddr,
    pub name: Option<String>,
    pub nud_state: Option<wakey_core::NeighborState>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LegacyWakeResult {
    pub result: Vec<LegacyWakeResultRow>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct LegacyWakeResultRow {
    #[serde(flatten)]
    pub target: wakey_core::WakeTarget,
    pub status: wakey_core::WakeStatus,
}

pub fn legacy_status_from_domain(status: Status<NeighborEntry>) -> LegacyStatusResponse {
    LegacyStatusResponse {
        name: status.name,
        table: status.table.into_iter().map(legacy_status_row).collect(),
        filters: status.filters,
    }
}

pub fn legacy_status_row(row: NeighborEntry) -> LegacyStatusRow {
    LegacyStatusRow {
        ip: row.ip,
        dev: row.dev,
        mac: row.mac,
        state: row.state,
    }
}

pub fn legacy_leases_from_domain(leases: Vec<DhcpLeaseWithState>) -> Vec<LegacyLeaseRow> {
    leases.into_iter().map(legacy_lease_row).collect()
}

pub fn legacy_lease_row(lease: DhcpLeaseWithState) -> LegacyLeaseRow {
    LegacyLeaseRow {
        expires_epoch: lease.lease_line.expires_epoch,
        ip: lease.lease_line.ip,
        mac: lease.lease_line.mac,
        name: lease.lease_line.name,
        nud_state: lease.nud_state,
    }
}

pub fn legacy_wake_from_domain(result: WakeResult) -> LegacyWakeResult {
    LegacyWakeResult {
        result: result.result.into_iter().map(legacy_wake_row).collect(),
    }
}

pub fn legacy_wake_row(row: WakeTargetResult) -> LegacyWakeResultRow {
    LegacyWakeResultRow {
        target: row.target,
        status: row.status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use wakey_core::{DhcpLease, NeighborState, WakeStatus, WakeTarget};

    #[test]
    fn maps_status_to_legacy_shape() {
        let status = Status {
            name: Some("pc".into()),
            table: vec![NeighborEntry {
                ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
                dev: Some("br-lan".into()),
                mac: Some("aa:bb:cc:dd:ee:ff".parse().expect("mac")),
                state: NeighborState::Reachable,
            }],
            filters: DeviceFilters::default(),
        };
        let legacy = legacy_status_from_domain(status);
        assert_eq!(legacy.name.as_deref(), Some("pc"));
        assert_eq!(legacy.table.len(), 1);
        assert_eq!(legacy.table[0].state, NeighborState::Reachable);
    }

    #[test]
    fn maps_leases_to_legacy_shape() {
        let leases = vec![DhcpLeaseWithState {
            lease_line: DhcpLease {
                expires_epoch: 42,
                ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
                mac: "aa:bb:cc:dd:ee:ff".parse().expect("mac"),
                name: Some("pc".into()),
            },
            nud_state: Some(NeighborState::Reachable),
        }];
        let legacy = legacy_leases_from_domain(leases);
        assert_eq!(legacy.len(), 1);
        assert_eq!(legacy[0].name.as_deref(), Some("pc"));
        assert_eq!(legacy[0].nud_state, Some(NeighborState::Reachable));
    }

    #[test]
    fn maps_wake_to_legacy_shape() {
        let result = WakeResult {
            result: vec![WakeTargetResult {
                target: WakeTarget {
                    ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                    mac: Some("aa:bb:cc:dd:ee:ff".parse().expect("mac")),
                },
                status: WakeStatus::Succeed,
            }],
        };
        let legacy = legacy_wake_from_domain(result);
        assert_eq!(legacy.result.len(), 1);
        assert_eq!(legacy.result[0].status, WakeStatus::Succeed);
    }
}
