use std::collections::HashSet;

use anyhow::Result;
use wakey_core::{
    Device, DeviceInventory, DhcpLease, DhcpLeaseWithState, InventoryQuery, NeighborEntry,
    Presence, Query,
};

use crate::service::leases::get_leases;
use crate::service::query::resolve_query;

/// Resolve free-form input and return merged devices rather than raw source rows.
pub async fn resolve_devices(input: impl Into<String>) -> Result<Vec<Device>> {
    let query = resolve_query(input).await?;
    inventory(query).await.map(|inventory| inventory.devices)
}

/// Build a merged device inventory from neighbor-table and DHCP-lease sources.
///
/// This is the current center of gravity for the service layer. Higher-level
/// status and wake flows should prefer deriving from inventory rather than
/// directly from raw Linux source rows.
pub async fn inventory(query: InventoryQuery) -> Result<DeviceInventory> {
    let neighbors = wakey_linux::devices::query_neighbors(&query).await?;
    let leases = get_leases(wakey_core::LeaseQuery {
        include_state: false,
    })
    .await?;
    Ok(DeviceInventory {
        devices: merge_devices(neighbors, leases, &query),
    })
}

/// Merge raw neighbor entries and DHCP leases into device aggregates.
///
/// Identity is currently MAC-first, with an IP-based fallback when a neighbor
/// row does not include a MAC address.
pub fn merge_devices(
    neighbors: Vec<NeighborEntry>,
    leases: Vec<DhcpLeaseWithState>,
    query: &InventoryQuery,
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

    let mut texts = HashSet::new();
    let mut devs = HashSet::new();
    let mut ips = HashSet::new();
    let mut macs = HashSet::new();
    let mut nuds = HashSet::new();

    for term in query {
        match term {
            Query::Text(v) => texts.insert(v.as_str()),
            Query::Interface(v) => devs.insert(v.as_str()),
            Query::Ip(v) => ips.insert(*v),
            Query::Mac(v) => macs.insert(*v),
            Query::NeighborState(v) => nuds.insert(*v),
        };
    }

    devices.retain(|device| {
        (texts.is_empty() || device.names.iter().any(|n| texts.contains(n.as_str())))
            && (devs.is_empty() || device.interfaces.iter().any(|i| devs.contains(i.as_str())))
            && (ips.is_empty() || device.ips.iter().any(|ip| ips.contains(ip)))
            && (macs.is_empty() || device.macs.iter().any(|mac| macs.contains(mac)))
            && (nuds.is_empty() || device.neighbors.iter().any(|n| nuds.contains(&n.state)))
    });
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use wakey_core::{DhcpLease, InventoryQueryBuilder, NeighborState};

    fn sample_neighbors() -> Vec<NeighborEntry> {
        vec![NeighborEntry {
            ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
            dev: Some("br-lan".to_string()),
            mac: Some("aa:bb:cc:dd:ee:ff".parse().expect("mac")),
            state: NeighborState::Reachable,
        }]
    }

    fn sample_leases() -> Vec<DhcpLeaseWithState> {
        vec![DhcpLeaseWithState {
            lease_line: DhcpLease {
                expires_epoch: 1,
                ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
                mac: "aa:bb:cc:dd:ee:ff".parse().expect("mac"),
                name: Some("pc".to_string()),
            },
            nud_state: None,
        }]
    }

    #[test]
    fn merge_devices_applies_and_across_categories() {
        let query = InventoryQueryBuilder::new()
            .maybe_text(Some("pc".to_string()))
            .interfaces(vec!["br-lan".to_string()])
            .neighbor_states(vec![NeighborState::Reachable])
            .build();

        let out = merge_devices(sample_neighbors(), sample_leases(), &query);
        assert_eq!(out.len(), 1);

        let no_match_query = InventoryQueryBuilder::new()
            .maybe_text(Some("pc".to_string()))
            .interfaces(vec!["eth9".to_string()])
            .build();
        let out = merge_devices(sample_neighbors(), sample_leases(), &no_match_query);
        assert!(out.is_empty());
    }

    #[test]
    fn merge_devices_allows_or_within_same_category() {
        let query = InventoryQueryBuilder::new()
            .neighbor_states(vec![NeighborState::Stale, NeighborState::Reachable])
            .build();

        let out = merge_devices(sample_neighbors(), sample_leases(), &query);
        assert_eq!(out.len(), 1);
    }
}
