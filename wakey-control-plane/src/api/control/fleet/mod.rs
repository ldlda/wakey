use std::collections::{BTreeSet, HashMap};

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use tracing::warn;
use wakey_agent::protocol::{AgentCommand, InventoryRequest, WakeRequest};

use crate::api::commands::relay_agent_command;
use crate::api::json_error;
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

pub async fn list_fleet_devices(
    State(state): State<AppState>,
    Query(query): Query<ListFleetDevicesQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match load_fleet_devices(&state, &query).await {
        Ok(devices) => Ok((StatusCode::OK, Json(devices))),
        Err(err) => {
            warn!(error = %err, "failed to list fleet devices");
            Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "list_fleet_devices_failed",
                &err.to_string(),
            ))
        }
    }
}

pub async fn refresh_fleet_devices(
    State(state): State<AppState>,
    Json(req): Json<RefreshFleetDevicesRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let mut agent_ids = if req.agent_ids.is_empty() {
        let sessions = state.sessions.read().await;
        sessions.keys().cloned().collect::<Vec<_>>()
    } else {
        req.agent_ids.clone()
    };
    agent_ids.sort();
    agent_ids.dedup();

    let mut results = Vec::with_capacity(agent_ids.len());
    let mut total_accepted = 0usize;
    for agent_id in agent_ids {
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

        match response {
            Ok(response) if response.status == "ok" => {
                let Some(result) = response.result else {
                    results.push(RefreshFleetAgentResult {
                        agent_id,
                        status: "error".into(),
                        accepted: 0,
                        error: Some("inventory command returned no result".into()),
                    });
                    continue;
                };
                match serde_json::from_value::<Vec<wakey_core::Device>>(
                    result
                        .get("devices")
                        .cloned()
                        .unwrap_or(serde_json::Value::Array(vec![])),
                ) {
                    Ok(devices) => {
                        match state
                            .store
                            .replace_agent_device_snapshot(&agent_id, &devices)
                            .await
                        {
                            Ok(accepted) => {
                                total_accepted = total_accepted.saturating_add(accepted);
                                results.push(RefreshFleetAgentResult {
                                    agent_id,
                                    status: "ok".into(),
                                    accepted,
                                    error: None,
                                });
                            }
                            Err(err) => results.push(RefreshFleetAgentResult {
                                agent_id,
                                status: "error".into(),
                                accepted: 0,
                                error: Some(err.to_string()),
                            }),
                        }
                    }
                    Err(err) => results.push(RefreshFleetAgentResult {
                        agent_id,
                        status: "error".into(),
                        accepted: 0,
                        error: Some(err.to_string()),
                    }),
                }
            }
            Ok(response) => results.push(RefreshFleetAgentResult {
                agent_id,
                status: "error".into(),
                accepted: 0,
                error: response
                    .error
                    .map(|error| error.message)
                    .or_else(|| Some("inventory command failed".into())),
            }),
            Err((status, body)) => results.push(RefreshFleetAgentResult {
                agent_id,
                status: "error".into(),
                accepted: 0,
                error: Some(format!("{status}: {}", body.0)),
            }),
        }
    }

    Ok((
        StatusCode::OK,
        Json(RefreshFleetDevicesResponse {
            total_accepted,
            agents: results,
        }),
    ))
}

pub async fn wake_fleet_device(
    State(state): State<AppState>,
    Json(req): Json<WakeFleetDeviceRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
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
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "load_fleet_devices_failed",
            &err.to_string(),
        )
    })?;

    let device = devices
        .into_iter()
        .find(|device| device.device_key == req.device_key)
        .ok_or_else(|| {
            json_error(
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
        json_error(
            StatusCode::BAD_REQUEST,
            "wake_route_unavailable",
            "no wakeable connected MAC-backed route is available",
        )
    })?;

    let Some(mac) = route.mac else {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "wake_route_unavailable",
            "selected route does not include a MAC address",
        ));
    };
    if !route.connected {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "wake_route_unavailable",
            "selected route agent is not connected",
        ));
    }
    if !route.wakeable {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "wake_route_unavailable",
            "selected route is not wakeable",
        ));
    }

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

    Ok((
        StatusCode::OK,
        Json(WakeFleetDeviceResponse { route, command }),
    ))
}

async fn load_fleet_devices(
    state: &AppState,
    query: &ListFleetDevicesQuery,
) -> anyhow::Result<Vec<FleetDevice>> {
    let known_devices = state.store.list_known_devices().await?;
    let agent_devices = state.store.list_agent_device_rows().await?;
    let connected = {
        let sessions = state.sessions.read().await;
        sessions.keys().cloned().collect::<BTreeSet<_>>()
    };
    let agents = state.store.list_agents_with_nicknames().await;
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
    let mut devices = build_fleet_devices(known_devices, agent_devices, &context);
    filter_fleet_devices(&mut devices, query);
    let limit = query.limit.unwrap_or(500).clamp(1, 1000);
    devices.truncate(limit);
    Ok(devices)
}
