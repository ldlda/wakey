//! Build and rank agent-specific wake routes without promoting IPs into summaries.

use super::{AgentRuntimeStatus, FleetAccumulator, FleetDeviceEndpoint, FleetWakeRoute};
use crate::state::AgentDeviceWithChildren;
use macaddr::MacAddr;
use std::net::IpAddr;
use wakey_core::EndpointSource;

/// Endpoint evidence is preserved even when its IP is excluded from the summary.
pub(super) fn add_endpoint_routes(
    entry: &mut FleetAccumulator,
    agent_device: &AgentDeviceWithChildren,
    status: &AgentRuntimeStatus,
) {
    let agent_id = &agent_device.device.agent_id;
    let first_seen = agent_device.device.first_seen();
    let last_seen = agent_device.device.last_seen();
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
            agent_id,
            endpoint.key.mac.as_ref(),
            endpoint.key.ip.as_ref(),
        );
        let wakeable = status.connected && endpoint.key.mac.is_some();
        insert_route(
            entry,
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
}

/// Compatibility for stored rows without authoritative endpoint evidence.
pub(super) fn add_legacy_routes(
    entry: &mut FleetAccumulator,
    agent_device: &AgentDeviceWithChildren,
    status: &AgentRuntimeStatus,
) {
    entry.sources.insert("device".to_string());
    let agent_id = &agent_device.device.agent_id;
    let device_presence = agent_device.presence();
    let last_seen = agent_device.device.last_seen();
    if !agent_device.macs.is_empty() {
        for mac in &agent_device.macs {
            let ip_for_mac = agent_device.ips.first().copied();
            let hostname_for_mac = agent_device.hostnames.first().cloned();
            let rid = route_id(agent_id, Some(mac), ip_for_mac.as_ref());
            let wakeable = status.connected;
            insert_route(
                entry,
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

    if agent_device.macs.is_empty()
        && let Some(ip) = agent_device.ips.first()
    {
        let hostname = agent_device.hostnames.first().cloned();
        let rid = route_id(agent_id, None, Some(ip));
        insert_route(
            entry,
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

fn insert_route(entry: &mut FleetAccumulator, candidate: FleetWakeRoute) {
    match entry.routes.entry(candidate.route_id.clone()) {
        std::collections::btree_map::Entry::Vacant(slot) => {
            slot.insert(candidate);
        }
        std::collections::btree_map::Entry::Occupied(mut slot) => {
            if compare_route_quality(&candidate, slot.get()).is_gt() {
                slot.insert(candidate);
            }
        }
    }
}

pub(super) fn compare_route_quality(a: &FleetWakeRoute, b: &FleetWakeRoute) -> std::cmp::Ordering {
    a.connected
        .cmp(&b.connected)
        .then_with(|| a.wakeable.cmp(&b.wakeable))
        .then_with(|| a.ip.is_some().cmp(&b.ip.is_some()))
        .then_with(|| a.presence.cmp(&b.presence))
        .then_with(|| a.last_seen_unix.cmp(&b.last_seen_unix))
        .then_with(|| source_quality_rank(&a.source).cmp(&source_quality_rank(&b.source)))
}

fn endpoint_source_label(source: EndpointSource) -> &'static str {
    match source {
        EndpointSource::Neighbor => "neighbor",
        EndpointSource::DhcpLease => "dhcp_lease",
        EndpointSource::HookNeighbor => "hook_neighbor",
        EndpointSource::HookDhcp => "hook_dhcp",
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

fn route_id(agent_id: &str, mac: Option<&MacAddr>, ip: Option<&IpAddr>) -> String {
    format!(
        "{}|{}|{}",
        agent_id,
        mac.map(|m| m.to_string()).unwrap_or_default(),
        ip.map(|i| i.to_string()).unwrap_or_default()
    )
}
