use std::collections::BTreeSet;
use std::net::IpAddr;

use serde::Deserialize;

use crate::state::AgentDeviceObservationInput;

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

pub(crate) fn inventory_result_to_observations(
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

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}
