use anyhow::Result;
use wakey_core::{Device, DeviceQuery, NeighborEntry, Presence, Status};

use crate::service::inventory::inventory;
use crate::service::query::resolve_query;

pub type StatusResponse = Status<NeighborEntry>;

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

pub fn device_to_status_rows(device: &Device) -> Vec<NeighborEntry> {
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
