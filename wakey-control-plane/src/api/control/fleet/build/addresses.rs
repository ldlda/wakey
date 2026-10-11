//! Current-address summaries and naming-only evidence have different eligibility.

use super::FleetAccumulator;
use crate::state::AgentDeviceWithChildren;
use wakey_core::{EndpointSource, Presence};

/// Keep naming evidence distinct from addresses eligible for the summary.
pub(super) fn add_address_evidence(
    entry: &mut FleetAccumulator,
    agent_device: &AgentDeviceWithChildren,
) {
    // Retain address evidence for naming only, without promoting stale IPs into
    // the active summary or wake-route candidates.
    let fallback_ip = agent_device
        .ips
        .iter()
        .copied()
        .chain(
            agent_device
                .endpoints
                .iter()
                .filter_map(|endpoint| endpoint.key.ip),
        )
        .min();
    entry.fallback_ip = entry.fallback_ip.into_iter().chain(fallback_ip).min();
    if agent_device.endpoints.is_empty() {
        for mac in &agent_device.macs {
            entry.macs.insert(*mac);
        }
        if agent_device.presence() != Presence::Offline {
            for ip in &agent_device.ips {
                entry.ips.insert(*ip);
            }
        }
    } else {
        for endpoint in &agent_device.endpoints {
            if let Some(mac) = endpoint.key.mac {
                entry.macs.insert(mac);
            }
            if endpoint_ip_is_summary_eligible(endpoint.key.source, endpoint.presence)
                && let Some(ip) = endpoint.key.ip
            {
                entry.ips.insert(ip);
            }
        }
    }
}

fn endpoint_ip_is_summary_eligible(source: EndpointSource, presence: Presence) -> bool {
    match source {
        EndpointSource::DhcpLease => true,
        EndpointSource::Neighbor => presence != Presence::Offline,
        EndpointSource::HookNeighbor | EndpointSource::HookDhcp => false,
    }
}

impl FleetAccumulator {
    /// Names and MACs outrank address evidence; presence belongs in the badge.
    /// Summary IPs outrank naming-only fallback IPs, which may be inactive.
    pub(super) fn display_label(&self) -> String {
        self.display_name
            .clone()
            .or_else(|| self.hostnames.iter().next().cloned())
            .or_else(|| {
                self.macs
                    .iter()
                    .next()
                    .map(|mac| mac.to_string())
                    .or_else(|| {
                        self.ips
                            .iter()
                            .next()
                            .copied()
                            .or(self.fallback_ip)
                            .map(|ip| ip.to_string())
                    })
                    .map(|address| format!("Device: {address}"))
            })
            .unwrap_or_else(|| "(unknown device)".to_string())
    }
}
