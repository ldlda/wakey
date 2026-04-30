use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::IpAddr;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use tracing::warn;
use wakey_agent::protocol::{AgentCommand, InventoryRequest, WakeRequest};

use crate::api::commands::{RelayCommandResponse, relay_agent_command};
use crate::api::json_error;
use crate::runtime::AppState;
use crate::state::{
    AgentDeviceObservation, AgentDeviceObservationInput, DeviceIdentifier, KnownDevice,
    KnownDeviceSummary,
};

#[derive(Debug, Deserialize)]
pub struct ListFleetDevicesQuery {
    pub query: Option<String>,
    pub presence: Option<String>,
    pub known: Option<String>,
    pub agent_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct RefreshFleetDevicesRequest {
    #[serde(default)]
    pub agent_ids: Vec<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RefreshFleetDevicesResponse {
    pub total_accepted: usize,
    pub agents: Vec<RefreshFleetAgentResult>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RefreshFleetAgentResult {
    pub agent_id: String,
    pub status: String,
    pub accepted: usize,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WakeFleetDeviceRequest {
    pub device_key: String,
    pub route_id: Option<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct WakeFleetDeviceResponse {
    pub route: FleetWakeRoute,
    pub command: RelayCommandResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetDevice {
    pub device_key: String,
    pub display_name: String,
    pub known_device: Option<KnownDeviceSummary>,
    pub pinned: bool,
    pub ips: Vec<String>,
    pub macs: Vec<String>,
    pub hostnames: Vec<String>,
    pub agents: Vec<FleetDeviceAgent>,
    pub sources: Vec<String>,
    pub first_seen_unix: Option<u64>,
    pub last_seen_unix: Option<u64>,
    pub presence: String,
    pub route_candidates: Vec<FleetWakeRoute>,
    pub recommended_route: Option<FleetWakeRoute>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetDeviceAgent {
    pub agent_id: String,
    pub nickname: Option<String>,
    pub connected: bool,
    pub last_seen_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetWakeRoute {
    pub route_id: String,
    pub agent_id: String,
    pub nickname: Option<String>,
    pub connected: bool,
    pub mac: Option<String>,
    pub ip: Option<String>,
    pub hostname: Option<String>,
    pub source: String,
    pub last_seen_unix: u64,
    pub wakeable: bool,
}

#[derive(Debug, Default)]
struct FleetBuildContext {
    agent_status: HashMap<String, AgentRuntimeStatus>,
    identifier_map: HashMap<String, KnownDeviceSummary>,
}

#[derive(Debug, Clone)]
struct AgentRuntimeStatus {
    nickname: Option<String>,
    connected: bool,
}

#[derive(Debug, Default)]
struct FleetAccumulator {
    device_key: String,
    display_name: Option<String>,
    known_device: Option<KnownDeviceSummary>,
    pinned: bool,
    ips: BTreeSet<String>,
    macs: BTreeSet<String>,
    hostnames: BTreeSet<String>,
    sources: BTreeSet<String>,
    agents: BTreeMap<String, FleetDeviceAgent>,
    first_seen_unix: Option<u64>,
    last_seen_unix: Option<u64>,
    presence_rank: u8,
    routes: BTreeMap<String, FleetWakeRoute>,
}

#[derive(Debug, Deserialize)]
struct InventoryEnvelope {
    kind: String,
    devices: Vec<InventoryDevice>,
}

#[derive(Debug, Deserialize)]
struct InventoryDevice {
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    ips: Vec<IpAddr>,
    #[serde(default)]
    macs: Vec<String>,
    #[serde(default)]
    neighbors: Vec<InventoryNeighbor>,
    #[serde(default)]
    leases: Vec<InventoryLease>,
    #[serde(default)]
    observations: Vec<InventoryObservationFact>,
    #[serde(default)]
    presence: String,
}

#[derive(Debug, Deserialize)]
struct InventoryNeighbor {
    ip: IpAddr,
    #[serde(default)]
    mac: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InventoryLease {
    ip: IpAddr,
    mac: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InventoryObservationFact {
    kind: String,
    action: String,
    #[serde(default)]
    mac: Option<String>,
    #[serde(default)]
    ip: Option<IpAddr>,
    #[serde(default)]
    hostname: Option<String>,
}

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
                match inventory_result_to_observations(result) {
                    Ok(observations) => match state
                        .store
                        .upsert_agent_observations(&agent_id, observations)
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
                    },
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

    let Some(mac) = route.mac.as_deref() else {
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

    let mac = mac.parse().map_err(|err| {
        json_error(
            StatusCode::BAD_REQUEST,
            "invalid_wake_route",
            &format!("invalid route MAC: {err}"),
        )
    })?;
    let ip = route
        .ip
        .as_deref()
        .map(str::parse)
        .transpose()
        .map_err(|err| {
            json_error(
                StatusCode::BAD_REQUEST,
                "invalid_wake_route",
                &format!("invalid route IP: {err}"),
            )
        })?;

    let command = relay_agent_command(
        &state,
        &route.agent_id,
        AgentCommand::Wake(WakeRequest {
            query: None,
            mac: Some(mac),
            ip,
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
    let observations = state
        .store
        .list_agent_observations(None, query.limit.unwrap_or(1000).max(1))
        .await?;
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
    let mut devices = build_fleet_devices(known_devices, observations, &context);
    filter_fleet_devices(&mut devices, query);
    let limit = query.limit.unwrap_or(500).clamp(1, 1000);
    devices.truncate(limit);
    Ok(devices)
}

fn build_fleet_devices(
    known_devices: Vec<KnownDevice>,
    observations: Vec<AgentDeviceObservation>,
    context: &FleetBuildContext,
) -> Vec<FleetDevice> {
    let mut by_key = BTreeMap::<String, FleetAccumulator>::new();

    for device in known_devices {
        let key = format!("known:{}", device.device_id);
        let entry = by_key
            .entry(key.clone())
            .or_insert_with(|| FleetAccumulator {
                device_key: key,
                display_name: Some(device.display_name.clone()),
                known_device: Some(known_device_summary(&device)),
                pinned: device.pinned,
                presence_rank: 1,
                ..Default::default()
            });
        for identifier in device.identifiers {
            add_identifier_to_entry(entry, &identifier);
        }
    }

    for observation in observations {
        let key = observation_group_key(&observation, context);
        let entry = by_key
            .entry(key.clone())
            .or_insert_with(|| FleetAccumulator {
                device_key: key,
                ..Default::default()
            });
        add_observation_to_entry(entry, observation, context);
    }

    let mut devices = by_key
        .into_values()
        .map(FleetAccumulator::into_response)
        .collect::<Vec<_>>();
    devices.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then_with(|| b.known_device.is_some().cmp(&a.known_device.is_some()))
            .then_with(|| presence_rank(&b.presence).cmp(&presence_rank(&a.presence)))
            .then_with(|| b.last_seen_unix.cmp(&a.last_seen_unix))
            .then_with(|| a.display_name.cmp(&b.display_name))
    });
    devices
}

fn filter_fleet_devices(devices: &mut Vec<FleetDevice>, query: &ListFleetDevicesQuery) {
    let search = normalize_filter(query.query.as_deref());
    let presence = normalize_filter(query.presence.as_deref());
    let known = normalize_filter(query.known.as_deref());
    let agent_id = normalize_filter(query.agent_id.as_deref());

    devices.retain(|device| {
        if let Some(presence) = presence.as_deref()
            && presence != "all"
            && device.presence != presence
        {
            return false;
        }
        if let Some(known) = known.as_deref() {
            match known {
                "known" if device.known_device.is_none() => return false,
                "unknown" if device.known_device.is_some() => return false,
                _ => {}
            }
        }
        if let Some(agent_id) = agent_id.as_deref()
            && !device
                .agents
                .iter()
                .any(|agent| agent.agent_id.to_ascii_lowercase().contains(agent_id))
        {
            return false;
        }
        if let Some(search) = search.as_deref() {
            let mut haystack = vec![
                device.device_key.as_str(),
                device.display_name.as_str(),
                device.presence.as_str(),
            ];
            haystack.extend(device.ips.iter().map(String::as_str));
            haystack.extend(device.macs.iter().map(String::as_str));
            haystack.extend(device.hostnames.iter().map(String::as_str));
            haystack.extend(device.sources.iter().map(String::as_str));
            haystack.extend(device.agents.iter().map(|agent| agent.agent_id.as_str()));
            if !haystack
                .into_iter()
                .any(|value| value.to_ascii_lowercase().contains(search))
            {
                return false;
            }
        }
        true
    });
}

fn observation_group_key(
    observation: &AgentDeviceObservation,
    context: &FleetBuildContext,
) -> String {
    if let Some(summary) = observation_known_device(observation, context) {
        return format!("known:{}", summary.device_id);
    }
    if let Some(mac) = observation.mac.as_deref() {
        return format!("mac:{mac}");
    }
    if let Some(ip) = observation.ip.as_deref() {
        return format!("ip:{ip}");
    }
    observation.observation_key.clone()
}

fn add_observation_to_entry(
    entry: &mut FleetAccumulator,
    observation: AgentDeviceObservation,
    context: &FleetBuildContext,
) {
    let observation_offline = observation_is_offline(&observation);
    if let Some(summary) = observation_known_device(&observation, context)
        && entry.known_device.is_none()
    {
        entry.display_name = Some(summary.display_name.clone());
        entry.pinned = summary.pinned;
        entry.known_device = Some(summary);
    }
    if let Some(mac) = observation.mac.as_deref() {
        entry.macs.insert(mac.to_string());
    }
    if !observation_offline && let Some(ip) = observation.ip.as_deref() {
        entry.ips.insert(ip.to_string());
    }
    if let Some(hostname) = observation.hostname.as_deref() {
        if entry.display_name.is_none() {
            entry.display_name = Some(hostname.to_string());
        }
        entry.hostnames.insert(hostname.to_string());
    }
    entry.sources.insert(observation.kind.clone());
    entry.first_seen_unix = Some(
        entry
            .first_seen_unix
            .map(|current| current.min(observation.first_seen_unix))
            .unwrap_or(observation.first_seen_unix),
    );
    entry.last_seen_unix = Some(
        entry
            .last_seen_unix
            .map(|current| current.max(observation.last_seen_unix))
            .unwrap_or(observation.last_seen_unix),
    );
    entry.presence_rank = entry
        .presence_rank
        .max(observation_presence_rank(&observation));

    let status = context
        .agent_status
        .get(&observation.agent_id)
        .cloned()
        .unwrap_or(AgentRuntimeStatus {
            nickname: None,
            connected: false,
        });
    entry
        .agents
        .entry(observation.agent_id.clone())
        .and_modify(|agent| {
            agent.last_seen_unix = agent.last_seen_unix.max(observation.last_seen_unix);
            agent.connected = status.connected;
            agent.nickname = status.nickname.clone();
        })
        .or_insert(FleetDeviceAgent {
            agent_id: observation.agent_id.clone(),
            nickname: status.nickname.clone(),
            connected: status.connected,
            last_seen_unix: observation.last_seen_unix,
        });

    let route_id = route_id(
        &observation.agent_id,
        observation.mac.as_deref(),
        observation.ip.as_deref(),
        &observation.kind,
    );
    let wakeable = status.connected && observation.mac.is_some() && !observation_offline;
    entry.routes.insert(
        route_id.clone(),
        FleetWakeRoute {
            route_id: route_id.clone(),
            agent_id: observation.agent_id,
            nickname: status.nickname,
            connected: status.connected,
            mac: observation.mac,
            ip: observation.ip,
            hostname: observation.hostname,
            source: observation.kind,
            last_seen_unix: observation.last_seen_unix,
            wakeable,
        },
    );
    if let Some(route) = entry.routes.get_mut(&route_id) {
        route.wakeable = route.connected && route.mac.is_some() && !observation_offline;
    }
}

fn add_identifier_to_entry(entry: &mut FleetAccumulator, identifier: &DeviceIdentifier) {
    match identifier.kind.as_str() {
        "mac" => {
            entry.macs.insert(identifier.value.clone());
        }
        "ip" => {
            entry.ips.insert(identifier.value.clone());
        }
        _ => {}
    }
}

fn observation_known_device(
    observation: &AgentDeviceObservation,
    context: &FleetBuildContext,
) -> Option<KnownDeviceSummary> {
    observation
        .mac
        .as_deref()
        .and_then(|mac| context.identifier_map.get(&format!("mac:{mac}")).cloned())
        .or_else(|| {
            observation
                .ip
                .as_deref()
                .and_then(|ip| context.identifier_map.get(&format!("ip:{ip}")).cloned())
        })
}

fn known_device_summary(device: &KnownDevice) -> KnownDeviceSummary {
    KnownDeviceSummary {
        device_id: device.device_id.clone(),
        display_name: device.display_name.clone(),
        pinned: device.pinned,
    }
}

impl FleetAccumulator {
    fn into_response(self) -> FleetDevice {
        let mut route_candidates = self.routes.into_values().collect::<Vec<_>>();
        route_candidates.sort_by(|a, b| {
            b.connected
                .cmp(&a.connected)
                .then_with(|| b.wakeable.cmp(&a.wakeable))
                .then_with(|| b.last_seen_unix.cmp(&a.last_seen_unix))
                .then_with(|| a.agent_id.cmp(&b.agent_id))
        });
        let recommended_route = route_candidates
            .iter()
            .find(|route| route.wakeable)
            .cloned();
        let display_name = self
            .display_name
            .or_else(|| self.hostnames.iter().next().cloned())
            .or_else(|| self.macs.iter().next().cloned())
            .or_else(|| self.ips.iter().next().cloned())
            .unwrap_or_else(|| "(unknown device)".to_string());

        FleetDevice {
            device_key: self.device_key,
            display_name,
            known_device: self.known_device,
            pinned: self.pinned,
            ips: self.ips.into_iter().collect(),
            macs: self.macs.into_iter().collect(),
            hostnames: self.hostnames.into_iter().collect(),
            agents: self.agents.into_values().collect(),
            sources: self.sources.into_iter().collect(),
            first_seen_unix: self.first_seen_unix,
            last_seen_unix: self.last_seen_unix,
            presence: rank_presence(self.presence_rank).to_string(),
            route_candidates,
            recommended_route,
        }
    }
}

fn inventory_result_to_observations(
    result: serde_json::Value,
) -> anyhow::Result<Vec<AgentDeviceObservationInput>> {
    let envelope: InventoryEnvelope = serde_json::from_value(result)?;
    if envelope.kind != "inventory" {
        anyhow::bail!("expected inventory result, got {}", envelope.kind);
    }

    let now = now_unix();
    let mut out = Vec::new();
    for device in envelope.devices {
        let hostname = device
            .names
            .iter()
            .find(|name| !name.trim().is_empty())
            .cloned();
        let mut wrote_source_observation = false;
        for neighbor in &device.neighbors {
            if neighbor.mac.is_none() && device.macs.is_empty() {
                out.push(inventory_observation(
                    inventory_action_for_neighbor(neighbor),
                    None,
                    Some(neighbor.ip.to_string()),
                    hostname.clone(),
                    now,
                ));
                wrote_source_observation = true;
                continue;
            }

            let macs = neighbor
                .mac
                .iter()
                .chain(device.macs.iter())
                .collect::<BTreeSet<_>>();
            for mac in macs {
                out.push(inventory_observation(
                    inventory_action_for_neighbor(neighbor),
                    Some(mac.clone()),
                    Some(neighbor.ip.to_string()),
                    hostname.clone(),
                    now,
                ));
                wrote_source_observation = true;
            }
        }
        for observation in &device.observations {
            if observation.mac.is_none() && observation.ip.is_none() {
                continue;
            }
            out.push(inventory_observation(
                inventory_action_for_observation(observation),
                observation.mac.clone(),
                observation.ip.map(|ip| ip.to_string()),
                observation.hostname.clone().or_else(|| hostname.clone()),
                now,
            ));
            wrote_source_observation = true;
        }
        for lease in &device.leases {
            out.push(inventory_observation(
                "update",
                Some(lease.mac.clone()),
                Some(lease.ip.to_string()),
                lease.name.clone().or_else(|| hostname.clone()),
                now,
            ));
            wrote_source_observation = true;
        }
        if wrote_source_observation {
            continue;
        }

        let action = if device.presence == "offline" {
            "remove"
        } else {
            "update"
        };
        if !device.macs.is_empty() {
            for mac in device.macs {
                out.push(inventory_observation(
                    action,
                    Some(mac),
                    device.ips.first().map(ToString::to_string),
                    hostname.clone(),
                    now,
                ));
            }
        } else {
            for ip in &device.ips {
                out.push(inventory_observation(
                    action,
                    None,
                    Some(ip.to_string()),
                    hostname.clone(),
                    now,
                ));
            }
        }
    }
    Ok(out)
}

fn inventory_observation(
    action: &str,
    mac: Option<String>,
    ip: Option<String>,
    hostname: Option<String>,
    now: u64,
) -> AgentDeviceObservationInput {
    AgentDeviceObservationInput {
        kind: "inventory".into(),
        action: action.into(),
        mac,
        ip,
        hostname,
        first_seen_unix: now,
        last_seen_unix: now,
    }
}

fn inventory_action_for_neighbor(neighbor: &InventoryNeighbor) -> &'static str {
    if neighbor
        .state
        .as_deref()
        .is_some_and(|state| state.eq_ignore_ascii_case("FAILED"))
    {
        "remove"
    } else {
        "update"
    }
}

fn inventory_action_for_observation(observation: &InventoryObservationFact) -> &str {
    match observation.action.as_str() {
        "add" | "old" | "update" => "update",
        "remove" | "del" => "remove",
        _ if observation.kind == "neigh" => "update",
        _ => "update",
    }
}

fn observation_presence_rank(observation: &AgentDeviceObservation) -> u8 {
    match observation.last_action.as_str() {
        "remove" => 0,
        "add" | "old" | "update" => 2,
        _ => 1,
    }
}

fn observation_is_offline(observation: &AgentDeviceObservation) -> bool {
    observation.last_action == "remove"
}

fn rank_presence(rank: u8) -> &'static str {
    match rank {
        3 => "online",
        2 => "likely_online",
        0 => "offline",
        _ => "unknown",
    }
}

fn presence_rank(presence: &str) -> u8 {
    match presence {
        "online" => 3,
        "likely_online" => 2,
        "offline" => 0,
        _ => 1,
    }
}

fn route_id(agent_id: &str, mac: Option<&str>, ip: Option<&str>, source: &str) -> String {
    format!(
        "{}|{}|{}|{}",
        agent_id,
        source,
        mac.unwrap_or(""),
        ip.unwrap_or("")
    )
}

fn normalize_filter(value: Option<&str>) -> Option<String> {
    value
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(connected: &[&str]) -> FleetBuildContext {
        FleetBuildContext {
            agent_status: connected
                .iter()
                .map(|agent| {
                    (
                        (*agent).to_string(),
                        AgentRuntimeStatus {
                            nickname: None,
                            connected: true,
                        },
                    )
                })
                .collect(),
            identifier_map: HashMap::new(),
        }
    }

    fn observation(
        agent_id: &str,
        mac: Option<&str>,
        ip: Option<&str>,
        last_seen_unix: u64,
    ) -> AgentDeviceObservation {
        AgentDeviceObservation {
            observation_key: format!(
                "agent:{agent_id}:dhcp:{}",
                mac.map(|mac| format!("mac:{mac}"))
                    .or_else(|| ip.map(|ip| format!("ip:{ip}")))
                    .unwrap_or_default()
            ),
            agent_id: agent_id.into(),
            kind: "dhcp".into(),
            mac: mac.map(str::to_string),
            ip: ip.map(str::to_string),
            hostname: Some("lda".into()),
            first_seen_unix: 1,
            last_seen_unix,
            last_action: "update".into(),
        }
    }

    #[test]
    fn fleet_grouping_combines_same_mac_across_agents() {
        let devices = build_fleet_devices(
            Vec::new(),
            vec![
                observation(
                    "agent-a",
                    Some("aa:bb:cc:dd:ee:ff"),
                    Some("192.168.1.2"),
                    10,
                ),
                observation(
                    "agent-b",
                    Some("aa:bb:cc:dd:ee:ff"),
                    Some("192.168.2.2"),
                    20,
                ),
            ],
            &context(&["agent-a", "agent-b"]),
        );

        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].macs, vec!["aa:bb:cc:dd:ee:ff"]);
        assert_eq!(devices[0].agents.len(), 2);
        assert_eq!(
            devices[0]
                .recommended_route
                .as_ref()
                .map(|route| route.agent_id.as_str()),
            Some("agent-b")
        );
    }

    #[test]
    fn known_device_with_two_macs_absorbs_both_observation_groups() {
        let known = KnownDevice {
            device_id: "dev-1".into(),
            display_name: "lda".into(),
            pinned: true,
            created_at_unix: 1,
            updated_at_unix: 1,
            notes: None,
            identifiers: vec![
                DeviceIdentifier {
                    identifier_key: "mac:aa:bb:cc:dd:ee:01".into(),
                    device_id: "dev-1".into(),
                    kind: "mac".into(),
                    value: "aa:bb:cc:dd:ee:01".into(),
                    created_at_unix: 1,
                },
                DeviceIdentifier {
                    identifier_key: "mac:aa:bb:cc:dd:ee:02".into(),
                    device_id: "dev-1".into(),
                    kind: "mac".into(),
                    value: "aa:bb:cc:dd:ee:02".into(),
                    created_at_unix: 1,
                },
            ],
        };
        let mut ctx = context(&["agent-a"]);
        for identifier in &known.identifiers {
            ctx.identifier_map.insert(
                identifier.identifier_key.clone(),
                known_device_summary(&known),
            );
        }

        let devices = build_fleet_devices(
            vec![known],
            vec![
                observation("agent-a", Some("aa:bb:cc:dd:ee:01"), None, 10),
                observation("agent-a", Some("aa:bb:cc:dd:ee:02"), None, 20),
            ],
            &ctx,
        );

        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].device_key, "known:dev-1");
        assert_eq!(devices[0].macs.len(), 2);
    }

    #[test]
    fn ip_only_unknown_is_visible_but_not_wakeable() {
        let devices = build_fleet_devices(
            Vec::new(),
            vec![observation("agent-a", None, Some("192.168.1.2"), 10)],
            &context(&["agent-a"]),
        );

        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].ips, vec!["192.168.1.2"]);
        assert!(devices[0].recommended_route.is_none());
        assert!(!devices[0].route_candidates[0].wakeable);
    }

    #[test]
    fn offline_observation_does_not_advertise_current_ip_or_wake_route() {
        let mut offline = observation(
            "agent-a",
            Some("aa:bb:cc:dd:ee:ff"),
            Some("192.168.1.2"),
            20,
        );
        offline.kind = "neigh".into();
        offline.last_action = "remove".into();

        let devices = build_fleet_devices(Vec::new(), vec![offline], &context(&["agent-a"]));

        assert_eq!(devices.len(), 1);
        assert!(devices[0].ips.is_empty());
        assert_eq!(devices[0].presence, "offline");
        assert!(devices[0].recommended_route.is_none());
        assert!(!devices[0].route_candidates[0].wakeable);
    }

    #[test]
    fn inventory_result_maps_to_stored_observations() {
        let observations = inventory_result_to_observations(serde_json::json!({
            "kind": "inventory",
            "devices": [{
                "names": ["lda"],
                "ips": ["192.168.1.2"],
                "macs": ["aa:bb:cc:dd:ee:ff"],
                "presence": "likely_online"
            }]
        }))
        .expect("inventory should map");

        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].kind, "inventory");
        assert_eq!(observations[0].action, "update");
        assert_eq!(observations[0].mac.as_deref(), Some("aa:bb:cc:dd:ee:ff"));
    }

    #[test]
    fn inventory_result_preserves_neighbor_failed_ip_as_remove() {
        let observations = inventory_result_to_observations(serde_json::json!({
            "kind": "inventory",
            "devices": [{
                "names": ["lda"],
                "ips": ["192.168.1.2", "192.168.1.3"],
                "macs": ["aa:bb:cc:dd:ee:ff"],
                "neighbors": [
                    {
                        "ip": "192.168.1.2",
                        "mac": "aa:bb:cc:dd:ee:ff",
                        "state": "FAILED"
                    },
                    {
                        "ip": "192.168.1.3",
                        "mac": "aa:bb:cc:dd:ee:ff",
                        "state": "REACHABLE"
                    }
                ],
                "presence": "online"
            }]
        }))
        .expect("inventory should map");

        assert_eq!(observations.len(), 2);
        let removed = observations
            .iter()
            .find(|observation| observation.ip.as_deref() == Some("192.168.1.2"))
            .expect("failed neighbor observation should exist");
        assert_eq!(removed.action, "remove");
        assert_eq!(removed.mac.as_deref(), Some("aa:bb:cc:dd:ee:ff"));

        let current = observations
            .iter()
            .find(|observation| observation.ip.as_deref() == Some("192.168.1.3"))
            .expect("reachable neighbor observation should exist");
        assert_eq!(current.action, "update");
        assert_eq!(current.mac.as_deref(), Some("aa:bb:cc:dd:ee:ff"));
    }

    #[test]
    fn inventory_result_preserves_hook_observations_and_leases() {
        let observations = inventory_result_to_observations(serde_json::json!({
            "kind": "inventory",
            "devices": [{
                "names": ["lda"],
                "ips": ["192.168.1.2", "192.168.1.3"],
                "macs": ["aa:bb:cc:dd:ee:ff"],
                "observations": [{
                    "kind": "neigh",
                    "action": "remove",
                    "mac": "aa:bb:cc:dd:ee:ff",
                    "ip": "192.168.1.2"
                }],
                "leases": [{
                    "expires_epoch": 1893456000_u64,
                    "ip": "192.168.1.3",
                    "mac": "aa:bb:cc:dd:ee:ff",
                    "name": "lda"
                }],
                "presence": "likely_online"
            }]
        }))
        .expect("inventory should map");

        assert_eq!(observations.len(), 2);
        assert!(observations.iter().any(|observation| {
            observation.action == "remove"
                && observation.mac.as_deref() == Some("aa:bb:cc:dd:ee:ff")
                && observation.ip.as_deref() == Some("192.168.1.2")
        }));
        assert!(observations.iter().any(|observation| {
            observation.action == "update"
                && observation.mac.as_deref() == Some("aa:bb:cc:dd:ee:ff")
                && observation.ip.as_deref() == Some("192.168.1.3")
                && observation.hostname.as_deref() == Some("lda")
        }));
    }
}
