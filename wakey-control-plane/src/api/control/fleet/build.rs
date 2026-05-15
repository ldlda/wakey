use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::IpAddr;

use macaddr::MacAddr;
use wakey_core::Presence;

use crate::state::{AgentDeviceWithChildren, DeviceIdentifier, KnownDevice, KnownDeviceSummary};

use super::types::{
    FleetDevice, FleetDeviceAgent, FleetDeviceEndpoint, FleetWakeRoute, ListFleetDevicesQuery,
};

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

#[derive(Debug)]
struct FleetAccumulator {
    device_key: String,
    display_name: Option<String>,
    known_device: Option<KnownDeviceSummary>,
    pinned: bool,
    ips: BTreeSet<IpAddr>,
    macs: BTreeSet<MacAddr>,
    hostnames: BTreeSet<String>,
    sources: BTreeSet<String>,
    agents: BTreeMap<String, FleetDeviceAgent>,
    endpoints: Vec<FleetDeviceEndpoint>,
    first_seen_unix: Option<u64>,
    last_seen_unix: Option<u64>,
    presence: Presence,
    routes: BTreeMap<String, FleetWakeRoute>,
}

impl Default for FleetAccumulator {
    fn default() -> Self {
        Self {
            device_key: String::new(),
            display_name: None,
            known_device: None,
            pinned: false,
            ips: BTreeSet::new(),
            macs: BTreeSet::new(),
            hostnames: BTreeSet::new(),
            sources: BTreeSet::new(),
            agents: BTreeMap::new(),
            endpoints: Vec::new(),
            first_seen_unix: None,
            last_seen_unix: None,
            presence: Presence::Offline,
            routes: BTreeMap::new(),
        }
    }
}

pub(crate) fn build_fleet_devices(
    known_devices: Vec<KnownDevice>,
    agent_devices: Vec<AgentDeviceWithChildren>,
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
                ..Default::default()
            });
        for identifier in &device.identifiers {
            add_identifier_to_entry(entry, identifier);
        }
    }

    for agent_device in agent_devices {
        let key = device_group_key(&agent_device, context);
        let entry = by_key
            .entry(key.clone())
            .or_insert_with(|| FleetAccumulator {
                device_key: key,
                ..Default::default()
            });
        add_agent_device_to_entry(entry, agent_device, context);
    }

    let mut devices = by_key
        .into_values()
        .map(FleetAccumulator::into_response)
        .collect::<Vec<_>>();
    devices.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then_with(|| b.known_device.is_some().cmp(&a.known_device.is_some()))
            .then_with(|| b.presence.cmp(&a.presence))
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
            && device.presence.as_str() != presence
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
            let mut haystack: Vec<String> = vec![
                device.device_key.clone(),
                device.display_name.clone(),
                device.presence.as_str().to_string(),
            ];
            haystack.extend(device.ips.iter().map(|ip| ip.to_string()));
            haystack.extend(device.macs.iter().map(|mac| mac.to_string()));
            haystack.extend(device.hostnames.clone());
            haystack.extend(device.sources.clone());
            haystack.extend(device.agents.iter().map(|a| a.agent_id.clone()));
            if !haystack
                .iter()
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
        && device.presence == Presence::Offline
}

fn device_group_key(agent_device: &AgentDeviceWithChildren, context: &FleetBuildContext) -> String {
    if let Some(summary) = device_known_device(agent_device, context) {
        return format!("known:{}", summary.device_id);
    }
    if let Some(mac) = agent_device.macs.first() {
        return format!("mac:{mac}");
    }
    if let Some(ip) = agent_device.ips.first() {
        return format!("ip:{ip}");
    }
    agent_device.device.device_key.clone()
}

fn add_agent_device_to_entry(
    entry: &mut FleetAccumulator,
    agent_device: AgentDeviceWithChildren,
    context: &FleetBuildContext,
) {
    let device_offline = agent_device.device.presence() == Presence::Offline;
    if let Some(summary) = device_known_device(&agent_device, context)
        && entry.known_device.is_none()
    {
        if let Some(ref name) = agent_device.device.display_name {
            entry.display_name = Some(name.clone());
        }
        entry.pinned = summary.pinned;
        entry.known_device = Some(summary);
    }
    for mac in &agent_device.macs {
        entry.macs.insert(*mac);
    }
    if !device_offline {
        for ip in &agent_device.ips {
            entry.ips.insert(*ip);
        }
    }
    for hostname in &agent_device.hostnames {
        if entry.display_name.is_none() {
            entry.display_name = Some(hostname.clone());
        }
        entry.hostnames.insert(hostname.clone());
    }
    let first_seen = agent_device.device.first_seen();
    let last_seen = agent_device.device.last_seen();
    entry.first_seen_unix = Some(
        entry
            .first_seen_unix
            .map(|current| current.min(first_seen))
            .unwrap_or(first_seen),
    );
    entry.last_seen_unix = Some(
        entry
            .last_seen_unix
            .map(|current| current.max(last_seen))
            .unwrap_or(last_seen),
    );
    let device_presence = agent_device.device.presence();
    entry.presence = std::cmp::max(entry.presence, device_presence);

    let agent_id = agent_device.device.agent_id.clone();
    let status = context
        .agent_status
        .get(&agent_id)
        .cloned()
        .unwrap_or(AgentRuntimeStatus {
            nickname: None,
            connected: false,
        });
    entry
        .agents
        .entry(agent_id.clone())
        .and_modify(|agent| {
            agent.last_seen_unix = agent.last_seen_unix.max(last_seen);
            agent.connected = status.connected;
            agent.nickname = status.nickname.clone();
        })
        .or_insert(FleetDeviceAgent {
            agent_id: agent_id.clone(),
            nickname: status.nickname.clone(),
            connected: status.connected,
            last_seen_unix: last_seen,
        });

    if agent_device.endpoints.is_empty() {
        entry.sources.insert("device".to_string());
    }

    for endpoint in &agent_device.endpoints {
        let source = endpoint_source_label(endpoint.key.source).to_string();
        entry.sources.insert(source.clone());
        let endpoint_last_seen = endpoint.last_seen_unix.unwrap_or(last_seen);
        let endpoint_first_seen = endpoint.first_seen_unix.or(Some(first_seen));
        entry.endpoints.push(FleetDeviceEndpoint {
            agent_id: agent_id.clone(),
            nickname: status.nickname.clone(),
            connected: status.connected,
            source: source.clone(),
            mac: endpoint.key.mac,
            ip: endpoint.key.ip,
            hostname: endpoint.hostname.clone(),
            interface: endpoint.interface.clone(),
            presence: endpoint.presence,
            first_seen_unix: endpoint_first_seen,
            last_seen_unix: Some(endpoint_last_seen),
        });

        let rid = route_id(
            &agent_id,
            endpoint.key.mac.as_ref(),
            endpoint.key.ip.as_ref(),
            &source,
        );
        let wakeable = status.connected && endpoint.key.mac.is_some();
        entry.routes.insert(
            rid.clone(),
            FleetWakeRoute {
                route_id: rid,
                agent_id: agent_id.clone(),
                nickname: status.nickname.clone(),
                connected: status.connected,
                mac: endpoint.key.mac,
                ip: endpoint.key.ip,
                hostname: endpoint.hostname.clone(),
                interface: endpoint.interface.clone(),
                source,
                presence: endpoint.presence,
                last_seen_unix: endpoint_last_seen,
                wakeable,
            },
        );
    }

    if agent_device.endpoints.is_empty() && !agent_device.macs.is_empty() {
        for mac in &agent_device.macs {
            let ip_for_mac = agent_device.ips.first().copied();
            let hostname_for_mac = agent_device.hostnames.first().cloned();
            let rid = route_id(&agent_id, Some(mac), ip_for_mac.as_ref(), "device");
            let wakeable = status.connected;
            entry.routes.insert(
                rid.clone(),
                FleetWakeRoute {
                    route_id: rid,
                    agent_id: agent_id.clone(),
                    nickname: status.nickname.clone(),
                    connected: status.connected,
                    mac: Some(*mac),
                    ip: ip_for_mac,
                    hostname: hostname_for_mac,
                    interface: None,
                    source: "device".to_string(),
                    presence: device_presence,
                    last_seen_unix: last_seen,
                    wakeable,
                },
            );
        }
    }

    if agent_device.endpoints.is_empty()
        && agent_device.macs.is_empty()
        && let Some(ip) = agent_device.ips.first()
    {
        let hostname = agent_device.hostnames.first().cloned();
        let rid = route_id(&agent_id, None, Some(ip), "device");
        entry.routes.insert(
            rid.clone(),
            FleetWakeRoute {
                route_id: rid,
                agent_id: agent_id.clone(),
                nickname: status.nickname.clone(),
                connected: status.connected,
                mac: None,
                ip: Some(*ip),
                hostname,
                interface: None,
                source: "device".to_string(),
                presence: device_presence,
                last_seen_unix: last_seen,
                wakeable: false,
            },
        );
    }
}

fn add_identifier_to_entry(entry: &mut FleetAccumulator, identifier: &DeviceIdentifier) {
    match identifier.kind.as_str() {
        "mac" => {
            if let Ok(mac) = identifier.value.parse::<MacAddr>() {
                entry.macs.insert(mac);
            }
        }
        "ip" => {
            if let Ok(ip) = identifier.value.parse::<IpAddr>() {
                entry.ips.insert(ip);
            }
        }
        _ => {}
    }
}

fn device_known_device(
    agent_device: &AgentDeviceWithChildren,
    context: &FleetBuildContext,
) -> Option<KnownDeviceSummary> {
    agent_device
        .macs
        .first()
        .map(|mac| format!("mac:{}", mac.to_string().to_ascii_lowercase()))
        .and_then(|key| context.identifier_map.get(&key).cloned())
        .or_else(|| {
            agent_device
                .ips
                .first()
                .map(|ip| format!("ip:{ip}"))
                .and_then(|key| context.identifier_map.get(&key).cloned())
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
                .then_with(|| b.ip.is_some().cmp(&a.ip.is_some()))
                .then_with(|| b.presence.cmp(&a.presence))
                .then_with(|| b.last_seen_unix.cmp(&a.last_seen_unix))
                .then_with(|| source_quality_rank(&b.source).cmp(&source_quality_rank(&a.source)))
                .then_with(|| a.agent_id.cmp(&b.agent_id))
        });
        let recommended_route = route_candidates
            .iter()
            .find(|route| route.wakeable)
            .cloned();
        let display_name = self
            .display_name
            .or_else(|| self.hostnames.iter().next().cloned())
            .or_else(|| self.macs.iter().next().map(|mac| mac.to_string()))
            .or_else(|| self.ips.iter().next().map(|ip| ip.to_string()))
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
            endpoints: self.endpoints,
            first_seen_unix: self.first_seen_unix,
            last_seen_unix: self.last_seen_unix,
            presence: self.presence,
            route_candidates,
            recommended_route,
        }
    }
}

fn endpoint_source_label(source: wakey_core::EndpointSource) -> &'static str {
    match source {
        wakey_core::EndpointSource::Neighbor => "neighbor",
        wakey_core::EndpointSource::DhcpLease => "dhcp_lease",
        wakey_core::EndpointSource::HookNeighbor => "hook_neighbor",
        wakey_core::EndpointSource::HookDhcp => "hook_dhcp",
    }
}

fn source_quality_rank(source: &str) -> u8 {
    match source {
        "neighbor" => 4,
        "dhcp_lease" => 3,
        "hook_neighbor" => 2,
        "hook_dhcp" => 1,
        _ => 0,
    }
}

fn route_id(agent_id: &str, mac: Option<&MacAddr>, ip: Option<&IpAddr>, source: &str) -> String {
    format!(
        "{}|{}|{}|{}",
        agent_id,
        source,
        mac.map(|m| m.to_string()).unwrap_or_default(),
        ip.map(|i| i.to_string()).unwrap_or_default()
    )
}

fn normalize_filter(value: Option<&str>) -> Option<String> {
    value
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}
