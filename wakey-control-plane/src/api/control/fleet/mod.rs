use std::collections::{BTreeSet, HashMap};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use tracing::warn;

use crate::api::ApiError;
use crate::runtime::AppState;

mod build;
mod refresh;
mod types;
mod wake;
pub use refresh::refresh_fleet_devices;
pub use wake::wake_fleet_device;

#[cfg(test)]
mod tests;

use build::{
    AgentRuntimeStatus, FleetBuildContext, build_fleet_devices, filter_fleet_devices,
    known_device_summary,
};
use types::{FleetDevice, ListFleetDevicesQuery};

pub(crate) type FleetCache =
    crate::runtime::view_cache::ViewCache<(u64, BTreeSet<String>), Vec<FleetDevice>>;

async fn fleet_view(state: &AppState) -> anyhow::Result<std::sync::Arc<Vec<FleetDevice>>> {
    state
        .fleet
        .get_or_try_init(
            || async {
                let connected = state
                    .sessions
                    .read()
                    .await
                    .keys()
                    .cloned()
                    .collect::<BTreeSet<_>>();
                (state.store.fleet_revision(), connected)
            },
            || build_fleet_view(state),
        )
        .await
}

/// Count stored rows separately from grouped identities. Reject mixed revisions;
/// filtered UI results must never become the source of current-state metrics.
pub(crate) async fn sample_fleet_counts(
    state: &AppState,
) -> anyhow::Result<wakey_core::telemetry::FleetCounts> {
    for _ in 0..3 {
        let revision = state.store.fleet_revision();
        let mut counts = wakey_core::telemetry::FleetCounts::default();
        for (agent, _) in state.store.list_agents_with_nicknames().await? {
            counts.agents.entry(agent).or_default();
        }
        for row in state.store.list_agent_device_rows().await? {
            let agent = counts.agents.entry(row.device.agent_id).or_default();
            agent.devices += 1;
            agent.endpoints += row.endpoints.len() as u64;
        }
        let fleet = fleet_view(state).await?;
        counts.saved = fleet
            .iter()
            .filter(|device| device.known_device.is_some())
            .count() as u64;
        counts.unsaved = fleet.len() as u64 - counts.saved;
        if state.store.fleet_revision() == revision {
            return Ok(counts);
        }
    }
    Err(crate::runtime::view_cache::RebuildLimit.into())
}

fn fleet_error(err: anyhow::Error, fallback_code: &str) -> ApiError {
    if err.is::<crate::runtime::view_cache::RebuildLimit>() {
        return ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "fleet_busy",
            err.to_string(),
        );
    }
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        fallback_code,
        err.to_string(),
    )
}

#[tracing::instrument(name = "fleet_list", skip_all)]
pub async fn list_fleet_devices(
    State(state): State<AppState>,
    Query(query): Query<ListFleetDevicesQuery>,
) -> Result<impl IntoResponse, ApiError> {
    match load_fleet_devices(&state, &query).await {
        Ok(devices) => Ok((StatusCode::OK, Json(devices))),
        Err(err) => {
            warn!(error = %err, "failed to list fleet devices");
            Err(fleet_error(err, "list_fleet_devices_failed"))
        }
    }
}

/// Details are an exact identity lookup, not a filtered/paginated list lookup.
/// In particular, remembering an unsaved row must remain readable under its new key.
#[tracing::instrument(name = "fleet_get", skip_all)]
pub async fn get_fleet_device(
    State(state): State<AppState>,
    Path(device_key): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let devices = fleet_view(&state)
        .await
        .map_err(|err| fleet_error(err, "get_fleet_device_failed"))?;
    let device = devices
        .iter()
        .find(|device| device.device_key == device_key)
        .cloned()
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "fleet_device_not_found",
                "fleet device not found",
            )
        })?;
    Ok(Json(device))
}

#[tracing::instrument(name = "fleet_load", skip_all, fields(grouped = tracing::field::Empty, filtered = tracing::field::Empty, returned = tracing::field::Empty))]
async fn load_fleet_devices(
    state: &AppState,
    query: &ListFleetDevicesQuery,
) -> anyhow::Result<Vec<FleetDevice>> {
    let cached = fleet_view(state).await?;
    // Cache the complete view. Query-specific visibility, search and limits
    // must never affect what another client (or a wake request) can retrieve.
    let mut devices = cached.as_ref().clone();
    tracing::Span::current().record("grouped", devices.len());
    filter_fleet_devices(&mut devices, query);
    tracing::Span::current().record("filtered", devices.len());
    let limit = query.limit.unwrap_or(500).clamp(1, 1000);
    devices.truncate(limit);
    tracing::Span::current().record("returned", devices.len());
    Ok(devices)
}

#[tracing::instrument(name = "fleet_rebuild", skip_all)]
async fn build_fleet_view(state: &AppState) -> anyhow::Result<Vec<FleetDevice>> {
    let known_devices = state.store.list_known_devices().await?;
    let agent_devices = state.store.list_agent_device_rows().await?;
    let connected = {
        let sessions = state.sessions.read().await;
        sessions.keys().cloned().collect::<BTreeSet<_>>()
    };
    let agents = state.store.list_agents_with_nicknames().await?;
    let agent_status = agents
        .into_iter()
        .map(|(agent_id, nickname)| {
            let connected = connected.contains(&agent_id);
            (
                agent_id,
                AgentRuntimeStatus {
                    nickname,
                    connected,
                },
            )
        })
        .collect::<HashMap<_, _>>();

    let mut identifier_map = HashMap::new();
    for device in &known_devices {
        let summary = known_device_summary(device);
        for identifier in &device.identifiers {
            identifier_map.insert(identifier.identifier_key.clone(), summary.clone());
        }
    }

    let context = FleetBuildContext {
        agent_status,
        identifier_map,
    };
    Ok(build_fleet_devices(known_devices, agent_devices, &context))
}
