use anyhow::Result;
use tracing::{Instrument, debug, instrument, warn};
use wakey_core::{DeviceInventory, DeviceObservationFact, DhcpLease, InventoryQuery};

use crate::config::AgentConfig;

pub(super) async fn read_leases(config: &AgentConfig) -> std::io::Result<Vec<DhcpLease>> {
    wakey::wakey_linux::dhcp::read_dhcp_leases_with_names_from_paths(
        &config.dhcp_leases_path,
        &config.observation_store_path,
        &config.mac_name_cache_path,
    )
    .await
}

#[instrument(name = "agent_inventory", skip_all, fields(neighbors = tracing::field::Empty, leases = tracing::field::Empty, observations = tracing::field::Empty, devices = tracing::field::Empty))]
pub(super) async fn collect(
    query: InventoryQuery,
    config: &AgentConfig,
) -> Result<DeviceInventory> {
    // This configured path serves both commands and periodic snapshots; keep
    // phase spans here so neither caller bypasses source timing.
    let neighbors = wakey::wakey_linux::devices::query_neighbors(&query)
        .instrument(tracing::info_span!("inventory_neighbors"))
        .await?;
    tracing::Span::current().record("neighbors", neighbors.len());
    let leases = read_leases(config)
        .instrument(tracing::info_span!("inventory_dhcp_leases"))
        .await?;
    tracing::Span::current().record("leases", leases.len());
    let observations = read_hook_facts(config).await;
    tracing::Span::current().record("observations", observations.len());
    debug!(
        neighbors = neighbors.len(),
        leases = leases.len(),
        observations = observations.len(),
        "dispatching inventory with merged sources"
    );
    let inventory = DeviceInventory {
        devices: wakey::merge_devices_with_observations(
            neighbors,
            wakey::leases_without_state(leases),
            observations,
            &query,
        ),
    };
    tracing::Span::current().record("devices", inventory.devices.len());
    debug!(
        rows = inventory.devices.len(),
        "dispatched inventory command"
    );
    Ok(inventory)
}

async fn read_hook_facts(config: &AgentConfig) -> Vec<DeviceObservationFact> {
    // Hook storage is best effort: a failure must not discard live neighbors or leases.
    match wakey::wakey_linux::observations::prune_expired(
        &config.observation_store_path,
        config.observation_retention,
    )
    .instrument(tracing::info_span!("inventory_observation_prune"))
    .await
    {
        Ok(removed) if removed > 0 => {
            debug!(removed, "pruned stale local hook observations");
        }
        Ok(_) => {}
        Err(err) => {
            warn!(error = %err, "failed pruning stale local hook observations");
        }
    }
    match wakey::wakey_linux::observations::list_local_observations_from_path(
        &config.observation_store_path,
    )
    .instrument(tracing::info_span!("inventory_hook_store"))
    .await
    {
        Ok(observations) => observations
            .into_iter()
            .filter_map(wakey::local_observation_to_fact)
            .collect::<Vec<_>>(),
        Err(err) => {
            warn!(error = %err, "failed reading local hook observations for inventory command");
            Vec::new()
        }
    }
}
