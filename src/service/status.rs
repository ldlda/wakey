use anyhow::Result;
use tracing::{debug, instrument};
use wakey_core::{Device, DeviceQuery, NeighborEntry, Presence, Status};

use crate::service::inventory::inventory;
use crate::service::query::resolve_query;

/// Service status payload, still expressed in terms of legacy neighbor rows.
pub type StatusResponse = Status<NeighborEntry>;

/// Return status rows derived from the merged device inventory.
///
/// This keeps the old status response shape alive while the underlying model is
/// increasingly device-centered.
#[instrument(skip_all, fields(name = ?query.name))]
pub async fn get_status(query: DeviceQuery) -> Result<StatusResponse> {
    let inventory = inventory(query.clone()).await?;
    let table: Vec<NeighborEntry> = inventory
        .devices
        .iter()
        .flat_map(device_to_status_rows)
        .collect();
    debug!(rows = table.len(), devices = inventory.devices.len(), "built status response");
    Ok(Status {
        name: query.name,
        table,
        filters: query.filter,
    })
}

/// Convenience wrapper around [`get_status`] for free-form user input.
#[instrument(skip_all)]
pub async fn get_status_for_input(input: impl Into<String>) -> Result<StatusResponse> {
    let query = resolve_query(input).await?;
    get_status(query).await
}

/// Project a device aggregate back into legacy status rows.
///
/// If the device already has neighbor rows they are reused directly; otherwise a
/// fallback row is synthesized from the best available device data.
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
