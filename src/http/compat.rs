//! Compatibility types and mappers for the legacy HTTP/static client.
//!
//! These types intentionally preserve old JSON shapes expected by `/static`
//! while the core and service layers evolve underneath them. They do not define
//! the long-term domain model of the project.

use serde::Serialize;
use wakey_core::parse::mac;
use wakey_core::{
    Device, DeviceFilters, DeviceInventory, DhcpLeaseWithState, NeighborEntry, Status, WakeResult,
    WakeTargetResult,
};

/// Legacy status row shape expected by the old `/static` frontend.
#[derive(Debug, Clone, Serialize)]
pub struct LegacyStatusRow {
    pub ip: std::net::IpAddr,
    pub dev: Option<String>,
    #[serde(with = "mac::option_mac")]
    pub mac: Option<macaddr::MacAddr>,
    pub state: wakey_core::NeighborState,
}

/// Legacy status response shape expected by the old `/static` frontend.
#[derive(Debug, Clone, Serialize)]
pub struct LegacyStatusResponse {
    pub name: Option<String>,
    pub table: Vec<LegacyStatusRow>,
    pub filters: DeviceFilters,
}

/// Legacy DHCP lease row shape expected by the old `/static` frontend.
#[derive(Debug, Clone, Serialize)]
pub struct LegacyLeaseRow {
    pub expires_epoch: u64,
    pub ip: std::net::IpAddr,
    #[serde(with = "mac")]
    pub mac: macaddr::MacAddr,
    pub name: Option<String>,
    pub nud_state: Option<wakey_core::NeighborState>,
}

/// Legacy wake response wrapper expected by the old `/static` frontend.
#[derive(Debug, Clone, Serialize)]
pub struct LegacyWakeResult {
    pub result: Vec<LegacyWakeResultRow>,
}

/// Legacy per-target wake row shape.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct LegacyWakeResultRow {
    #[serde(flatten)]
    pub target: wakey_core::WakeTarget,
    pub status: wakey_core::WakeStatus,
}

/// Map legacy-style status rows into the old response shape.
///
/// This helper exists for compatibility with the original frontend contract.
pub fn legacy_status_from_domain(status: Status<NeighborEntry>) -> LegacyStatusResponse {
    LegacyStatusResponse {
        name: status.name,
        table: status.table.into_iter().map(legacy_status_row).collect(),
        filters: status.filters,
    }
}

/// Project a device inventory into the legacy status response shape.
pub fn legacy_status_from_inventory(
    inventory: DeviceInventory,
    name: Option<String>,
    filters: DeviceFilters,
) -> LegacyStatusResponse {
    let table = inventory
        .devices
        .into_iter()
        .flat_map(legacy_status_rows_from_device)
        .collect();
    LegacyStatusResponse {
        name,
        table,
        filters,
    }
}

/// Convert one neighbor row to the legacy status row shape.
pub fn legacy_status_row(row: NeighborEntry) -> LegacyStatusRow {
    LegacyStatusRow {
        ip: row.ip,
        dev: row.dev,
        mac: row.mac,
        state: row.state,
    }
}

/// Project one merged device back into legacy status rows.
pub fn legacy_status_rows_from_device(device: Device) -> Vec<LegacyStatusRow> {
    if !device.neighbors.is_empty() {
        return device
            .neighbors
            .into_iter()
            .map(legacy_status_row)
            .collect::<Vec<_>>();
    }

    let fallback_mac = device.macs.first().copied();
    let fallback_dev = device.interfaces.first().cloned();
    let fallback_state = match device.presence {
        wakey_core::Presence::Online => wakey_core::NeighborState::Reachable,
        wakey_core::Presence::LikelyOnline => wakey_core::NeighborState::Stale,
        wakey_core::Presence::Offline => wakey_core::NeighborState::Failed,
        wakey_core::Presence::Unknown => wakey_core::NeighborState::None,
    };

    device
        .ips
        .into_iter()
        .map(|ip| LegacyStatusRow {
            ip,
            dev: fallback_dev.clone(),
            mac: fallback_mac,
            state: fallback_state,
        })
        .collect()
}

/// Convert lease rows into the legacy frontend shape.
pub fn legacy_leases_from_domain(leases: Vec<DhcpLeaseWithState>) -> Vec<LegacyLeaseRow> {
    leases.into_iter().map(legacy_lease_row).collect()
}

/// Convert one lease row into the legacy frontend shape.
pub fn legacy_lease_row(lease: DhcpLeaseWithState) -> LegacyLeaseRow {
    LegacyLeaseRow {
        expires_epoch: lease.lease_line.expires_epoch,
        ip: lease.lease_line.ip,
        mac: lease.lease_line.mac,
        name: lease.lease_line.name,
        nud_state: lease.nud_state,
    }
}

/// Convert wake results into the legacy frontend shape.
pub fn legacy_wake_from_domain(result: WakeResult) -> LegacyWakeResult {
    LegacyWakeResult {
        result: result.result.into_iter().map(legacy_wake_row).collect(),
    }
}

/// Convert one wake result row into the legacy frontend shape.
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
    fn maps_inventory_to_legacy_status_shape() {
        let inventory = DeviceInventory {
            devices: vec![Device::from_parts(
                vec![NeighborEntry {
                    ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
                    dev: Some("br-lan".into()),
                    mac: Some("aa:bb:cc:dd:ee:ff".parse().expect("mac")),
                    state: NeighborState::Reachable,
                }],
                vec![],
            )],
        };
        let legacy =
            legacy_status_from_inventory(inventory, Some("pc".into()), DeviceFilters::default());
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
