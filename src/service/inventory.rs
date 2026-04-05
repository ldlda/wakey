use anyhow::Result;
use wakey_core::{
    Device, DeviceInventory, DeviceQuery, DhcpLease, DhcpLeaseWithState, NeighborEntry, Presence,
};

use crate::service::leases::get_leases;
use crate::service::query::resolve_query;

pub async fn resolve_devices(input: impl Into<String>) -> Result<Vec<Device>> {
    let query = resolve_query(input).await?;
    inventory(query).await.map(|inventory| inventory.devices)
}

pub async fn inventory(query: DeviceQuery) -> Result<DeviceInventory> {
    let neighbors = wakey_linux::devices::query_status(&query).await?;
    let leases = get_leases(wakey_core::LeaseQuery {
        include_state: false,
    })
    .await?;
    Ok(DeviceInventory {
        devices: merge_devices(neighbors, leases, &query),
    })
}

pub fn merge_devices(
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
