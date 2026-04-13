use anyhow::Result;
use tracing::{debug, instrument};
use wakey_core::{DeviceInventory, DeviceQuery};

use crate::service::inventory::inventory;
use crate::service::query::resolve_query;

/// Service status payload expressed in terms of merged device inventory.
pub type StatusResponse = DeviceInventory;

/// Return status payload from merged device inventory.
#[instrument(skip_all, fields(name = ?query.name))]
pub async fn get_status(query: DeviceQuery) -> Result<StatusResponse> {
    let inventory = inventory(query).await?;
    debug!(devices = inventory.devices.len(), "built status response");
    Ok(inventory)
}

/// Convenience wrapper around [`get_status`] for free-form user input.
#[instrument(skip_all)]
pub async fn get_status_for_input(input: impl Into<String>) -> Result<StatusResponse> {
    let query = resolve_query(input).await?;
    get_status(query).await
}
