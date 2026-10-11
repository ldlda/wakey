use std::collections::{BTreeSet, HashMap};

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use futures_util::StreamExt;
use tracing::{Instrument, info, warn};
use wakey_agent::protocol::{AgentCommand, InventoryRequest, WakeRequest};

use crate::api::ApiError;
use crate::api::commands::{RelayCommandResponse, relay_agent_command};
use crate::runtime::AppState;

mod build;
mod types;

#[cfg(test)]
mod tests;

use build::{
    AgentRuntimeStatus, FleetBuildContext, build_fleet_devices, filter_fleet_devices,
    known_device_summary,
};
use types::{
    FleetDevice, ListFleetDevicesQuery, RefreshFleetAgentResult, RefreshFleetDevicesRequest,
    RefreshFleetDevicesResponse, WakeFleetDeviceRequest, WakeFleetDeviceResponse,
};

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

const REFRESH_CONCURRENCY: usize = 8;

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

#[tracing::instrument(name = "fleet_refresh", skip_all, fields(agents = tracing::field::Empty, succeeded = tracing::field::Empty, failed = tracing::field::Empty, accepted = tracing::field::Empty))]
pub async fn refresh_fleet_devices(
    State(state): State<AppState>,
    Json(req): Json<RefreshFleetDevicesRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let agent_ids = if req.agent_ids.is_empty() {
        let sessions = state.sessions.read().await;
        sessions.keys().cloned().collect::<Vec<_>>()
    } else {
        req.agent_ids.clone()
    };
    let agent_ids = normalize_refresh_agent_ids(agent_ids);
    tracing::Span::current().record("agents", agent_ids.len());

    let results = futures_util::stream::iter(agent_ids.into_iter().map(|agent_id| {
        let state = state.clone();
        // Each concurrent future carries its own span through relay and storage.
        let span = tracing::info_span!("fleet_refresh_agent", agent_id = %agent_id, outcome = tracing::field::Empty, accepted = tracing::field::Empty);
        async move {
            let response = relay_agent_command(
                &state,
                &agent_id,
                AgentCommand::Inventory(InventoryRequest {
                    query: None,
                    name: None,
                    ips: Vec::new(),
                    devs: Vec::new(),
                    nuds: Vec::new(),
                    macs: Vec::new(),
                }),
                req.timeout_ms,
            )
            .await;
            let result = refresh_fleet_agent_result(&state, &agent_id, response).await;
            tracing::Span::current().record("outcome", result.status.as_str());
            tracing::Span::current().record("accepted", result.accepted);
            if result.status != "ok" {
                // The response retains diagnostic detail; do not export payloads.
                warn!("fleet agent refresh failed");
            }
            result
        }
        .instrument(span)
    }))
    .buffer_unordered(REFRESH_CONCURRENCY)
    .collect::<Vec<_>>()
    .await;

    let mut results = results;
    results.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
    let total_accepted = results.iter().map(|result| result.accepted).sum::<usize>();
    let succeeded = results
        .iter()
        .filter(|result| result.status == "ok")
        .count();
    let span = tracing::Span::current();
    span.record("succeeded", succeeded);
    span.record("failed", results.len() - succeeded);
    span.record("accepted", total_accepted);
    info!("fleet refresh completed");

    Ok((
        StatusCode::OK,
        Json(RefreshFleetDevicesResponse {
            total_accepted,
            agents: results,
        }),
    ))
}

fn normalize_refresh_agent_ids(mut agent_ids: Vec<String>) -> Vec<String> {
    agent_ids.sort();
    agent_ids.dedup();
    agent_ids
}

async fn refresh_fleet_agent_result(
    state: &AppState,
    agent_id: &str,
    response: Result<RelayCommandResponse, ApiError>,
) -> RefreshFleetAgentResult {
    match response {
        Ok(response) if response.status == "ok" => {
            let Some(result) = response.result else {
                return RefreshFleetAgentResult {
                    agent_id: agent_id.to_string(),
                    status: "error".into(),
                    accepted: 0,
                    error: Some("inventory command returned no result".into()),
                };
            };
            match serde_json::from_value::<wakey_core::DeviceInventory>(result)
                .map(|inventory| inventory.devices)
            {
                Ok(devices) => {
                    match state
                        .store
                        .replace_agent_device_snapshot(agent_id, &devices)
                        .await
                    {
                        Ok(accepted) => RefreshFleetAgentResult {
                            agent_id: agent_id.to_string(),
                            status: "ok".into(),
                            accepted,
                            error: None,
                        },
                        Err(err) => RefreshFleetAgentResult {
                            agent_id: agent_id.to_string(),
                            status: "error".into(),
                            accepted: 0,
                            error: Some(err.to_string()),
                        },
                    }
                }
                Err(err) => RefreshFleetAgentResult {
                    agent_id: agent_id.to_string(),
                    status: "error".into(),
                    accepted: 0,
                    error: Some(err.to_string()),
                },
            }
        }
        Ok(response) => RefreshFleetAgentResult {
            agent_id: agent_id.to_string(),
            status: "error".into(),
            accepted: 0,
            error: response
                .error
                .map(|error| error.message)
                .or_else(|| Some("inventory command failed".into())),
        },
        Err(err) => RefreshFleetAgentResult {
            agent_id: agent_id.to_string(),
            status: "error".into(),
            accepted: 0,
            error: Some(format!("{}: {}", err.status, err.message)),
        },
    }
}

#[tracing::instrument(name = "fleet_wake", skip_all, fields(explicit_route = req.route_id.is_some(), outcome = tracing::field::Empty))]
pub async fn wake_fleet_device(
    State(state): State<AppState>,
    Json(req): Json<WakeFleetDeviceRequest>,
) -> Result<impl IntoResponse, ApiError> {
    // Early database/relay errors retain an outcome even without a command result.
    tracing::Span::current().record("outcome", "error");
    let query = ListFleetDevicesQuery {
        query: None,
        presence: None,
        known: None,
        agent_id: None,
        visibility: Some("all".into()),
        limit: Some(1000),
    };
    let devices = load_fleet_devices(&state, &query).await.map_err(|err| {
        warn!(error = %err, "failed loading fleet devices for wake");
        fleet_error(err, "load_fleet_devices_failed")
    })?;

    let device = devices
        .into_iter()
        .find(|device| device.device_key == req.device_key)
        .ok_or_else(|| {
            tracing::Span::current().record("outcome", "device_not_found");
            ApiError::new(
                StatusCode::NOT_FOUND,
                "fleet_device_not_found",
                "fleet device not found",
            )
        })?;

    let route = match req.route_id.as_deref() {
        Some(route_id) => device
            .route_candidates
            .into_iter()
            .find(|route| route.route_id == route_id),
        None => device.recommended_route,
    }
    .ok_or_else(|| {
        tracing::Span::current().record("outcome", "route_unavailable");
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "wake_route_unavailable",
            "no wakeable connected MAC-backed route is available",
        )
    })?;

    let Some(mac) = route.mac else {
        tracing::Span::current().record("outcome", "route_without_mac");
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "wake_route_unavailable",
            "selected route does not include a MAC address",
        ));
    };
    if !route.connected {
        tracing::Span::current().record("outcome", "route_disconnected");
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "wake_route_unavailable",
            "selected route agent is not connected",
        ));
    }
    if !route.wakeable {
        tracing::Span::current().record("outcome", "route_not_wakeable");
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "wake_route_unavailable",
            "selected route is not wakeable",
        ));
    }

    info!(agent_id = %route.agent_id, has_ip = route.ip.is_some(), explicit_route = req.route_id.is_some(), "fleet wake route selected");
    let command = relay_agent_command(
        &state,
        &route.agent_id,
        AgentCommand::Wake(WakeRequest {
            query: None,
            mac: Some(mac),
            ip: route.ip,
        }),
        req.timeout_ms,
    )
    .await?;
    tracing::Span::current().record("outcome", command.status.as_str());

    Ok((
        StatusCode::OK,
        Json(WakeFleetDeviceResponse { route, command }),
    ))
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
