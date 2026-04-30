use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::state::{AgentDeviceObservation, DeviceIdentifier, KnownDevice, KnownDeviceSummary};

use super::types::{FleetDevice, FleetDeviceAgent, FleetWakeRoute, ListFleetDevicesQuery};

#[derive(Debug, Default)]
pub(crate) struct FleetBuildContext {
    pub(crate) agent_status: HashMap<String, AgentRuntimeStatus>,
    pub(crate) identifier_map: HashMap<String, KnownDeviceSummary>,
}

#[derive(Debug, Clone)]
pub(crate) struct AgentRuntimeStatus {
    pub(crate) nickname: Option<String>,
    pub(crate) connected: bool,
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

pub(crate) fn build_fleet_devices(
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

pub(crate) fn filter_fleet_devices(devices: &mut Vec<FleetDevice>, query: &ListFleetDevicesQuery) {
    let search = normalize_filter(query.query.as_deref());
    let presence = normalize_filter(query.presence.as_deref());
    let known = normalize_filter(query.known.as_deref());
    let agent_id = normalize_filter(query.agent_id.as_deref());
    let visibility = normalize_filter(query.visibility.as_deref());

    devices.retain(|device| {
        if visibility.as_deref().unwrap_or("operator") != "all"
            && fleet_device_is_operator_noise(device)
        {
            return false;
        }
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

fn fleet_device_is_operator_noise(device: &FleetDevice) -> bool {
    device.known_device.is_none()
        && device.macs.is_empty()
        && device.hostnames.is_empty()
        && device.recommended_route.is_none()
        && device.ips.is_empty()
        && device.presence == "offline"
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

pub(crate) fn known_device_summary(device: &KnownDevice) -> KnownDeviceSummary {
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
