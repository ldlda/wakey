use anyhow::Result;
use tracing::{debug, warn};
use wakey_core::{
    Device, DeviceInventory, DeviceObservationFact, DhcpLease, DhcpLeaseWithState, InventoryQuery,
    NeighborEntry, Presence, Query,
};

use crate::service::leases::get_leases;
use crate::service::query::resolve_query;

/// Resolve free-form input and return merged devices rather than raw source rows.
pub async fn resolve_devices(input: impl Into<String>) -> Result<Vec<Device>> {
    let query = resolve_query(input).await?;
    inventory(query).await.map(|inventory| inventory.devices)
}

/// Build a merged device inventory from neighbor-table and DHCP-lease sources.
///
/// This is the current center of gravity for the service layer. Higher-level
/// status and wake flows should prefer deriving from inventory rather than
/// directly from raw Linux source rows.
pub async fn inventory(query: InventoryQuery) -> Result<DeviceInventory> {
    let neighbors = wakey_linux::devices::query_neighbors(&query).await?;
    let leases = get_leases().await?;
    let observations = match wakey_linux::observations::list_local_observations().await {
        Ok(observations) => observations
            .into_iter()
            .filter_map(local_observation_to_fact)
            .collect::<Vec<_>>(),
        Err(err) => {
            warn!(error = %err, "failed reading local hook observations for inventory");
            Vec::new()
        }
    };
    debug!(
        neighbors = neighbors.len(),
        leases = leases.len(),
        observations = observations.len(),
        "building device inventory"
    );
    let devices = merge_devices_with_observations(neighbors, leases, observations, &query);
    debug!(devices = devices.len(), "merged device inventory");
    Ok(DeviceInventory { devices })
}

/// Merge raw neighbor entries and DHCP leases into device aggregates.
///
/// Identity is currently MAC-first, with an IP-based fallback when a neighbor
/// row does not include a MAC address.
pub fn merge_devices(
    neighbors: Vec<NeighborEntry>,
    leases: Vec<DhcpLeaseWithState>,
    query: &InventoryQuery,
) -> Vec<Device> {
    merge_devices_with_observations(neighbors, leases, Vec::new(), query)
}

/// Merge raw neighbor entries, DHCP leases, and hook observations into device
/// aggregates.
pub fn merge_devices_with_observations(
    neighbors: Vec<NeighborEntry>,
    leases: Vec<DhcpLeaseWithState>,
    observations: Vec<DeviceObservationFact>,
    query: &InventoryQuery,
) -> Vec<Device> {
    use std::collections::BTreeMap;

    type DeviceParts = (
        Vec<NeighborEntry>,
        Vec<DhcpLease>,
        Vec<DeviceObservationFact>,
    );

    let mut by_key: BTreeMap<String, DeviceParts> = BTreeMap::new();

    for row in neighbors {
        let key = row
            .mac
            .map(|m| m.to_string())
            .unwrap_or_else(|| format!("ip:{}", row.ip)); // key by mac or by FAILED ip
        by_key.entry(key).or_default().0.push(row);
    }
    for lease in leases {
        let key = lease.lease_line.mac.to_string();
        by_key.entry(key).or_default().1.push(lease.lease_line);
    }
    for observation in observations {
        let key = observation
            .mac
            .map(|mac| mac.to_string())
            .or_else(|| observation.ip.map(|ip| format!("ip:{ip}")))
            .unwrap_or_else(|| "unknown".to_string());
        by_key.entry(key).or_default().2.push(observation);
    }

    let mut devices: Vec<Device> = by_key
        .into_values()
        .map(|(neighbors, leases, observations)| {
            Device::from_parts_with_observations(neighbors, leases, observations)
        })
        .collect();
    if !query.is_empty() {
        let mut texts = Vec::new();
        let mut devs = Vec::new();
        let mut ips = Vec::new();
        let mut macs = Vec::new();
        let mut nuds = Vec::new();

        for term in query {
            match term {
                Query::Text(v) => texts.push(v.as_str()),
                Query::Interface(v) => devs.push(v.as_str()),
                Query::Ip(v) => ips.push(*v),
                Query::Mac(v) => macs.push(*v),
                Query::NeighborState(v) => nuds.push(*v),
            };
        }

        devices.retain(|device| {
            (texts.is_empty() || device.names.iter().any(|n| texts.contains(&n.as_str())))
            && (devs.is_empty() || device.interfaces.iter().any(|i| devs.contains(&i.as_str()))) // same pattern as iter().any
            && (ips.is_empty() || device.ips.iter().any(|ip| ips.contains(ip)))
            && (macs.is_empty() || device.macs.iter().any(|mac| macs.contains(mac)))
            && (nuds.is_empty() || device.neighbors.iter().any(|n| nuds.contains(&n.state)))
        });
    }
    devices.sort_by(|a, b| {
        presence_rank(b.presence)
            .cmp(&presence_rank(a.presence))
            .then_with(|| a.names.first().cmp(&b.names.first()))
    });
    devices
}

pub fn local_observation_to_fact(
    observation: wakey_linux::observations::LocalDeviceObservation,
) -> Option<DeviceObservationFact> {
    let mac = match observation.mac.as_deref() {
        Some(raw) => match raw.parse() {
            Ok(mac) => Some(mac),
            Err(err) => {
                warn!(
                    mac = raw,
                    error = %err,
                    "ignoring invalid observed mac in inventory"
                );
                None
            }
        },
        None => None,
    };
    if mac.is_none() && observation.ip.is_none() {
        warn!(
            kind = %observation.kind,
            action = %observation.action,
            "ignoring hook observation without mac or ip"
        );
        return None;
    }
    Some(DeviceObservationFact {
        kind: observation.kind,
        action: observation.action,
        mac,
        ip: observation.ip,
        hostname: observation.hostname,
        first_seen_unix: Some(observation.first_seen_unix),
        last_seen_unix: Some(observation.last_seen_unix),
    })
}

const fn presence_rank(presence: Presence) -> u8 {
    match presence {
        Presence::Online => 3,
        Presence::LikelyOnline => 2,
        Presence::Unknown => 1,
        Presence::Offline => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use wakey_core::{DeviceId, DhcpLease, EndpointSource, InventoryQueryBuilder, NeighborState};

    fn sample_neighbors() -> Vec<NeighborEntry> {
        vec![NeighborEntry {
            ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
            dev: Some("br-lan".to_string()),
            mac: Some("aa:bb:cc:dd:ee:ff".parse().expect("mac")),
            state: NeighborState::Reachable,
        }]
    }

    fn sample_leases() -> Vec<DhcpLeaseWithState> {
        vec![DhcpLeaseWithState {
            lease_line: DhcpLease {
                expires_epoch: 1,
                ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
                mac: "aa:bb:cc:dd:ee:ff".parse().expect("mac"),
                name: Some("pc".to_string()),
            },
            nud_state: None,
        }]
    }

    #[test]
    fn merge_devices_applies_and_across_categories() {
        let query = InventoryQueryBuilder::new()
            .maybe_text(Some("pc".to_string()))
            .interfaces(vec!["br-lan".to_string()])
            .neighbor_states(vec![NeighborState::Reachable])
            .build();

        let out = merge_devices(sample_neighbors(), sample_leases(), &query);
        assert_eq!(out.len(), 1);

        let no_match_query = InventoryQueryBuilder::new()
            .maybe_text(Some("pc".to_string()))
            .interfaces(vec!["eth9".to_string()])
            .build();
        let out = merge_devices(sample_neighbors(), sample_leases(), &no_match_query);
        assert!(out.is_empty());
    }

    #[test]
    fn merge_devices_allows_or_within_same_category() {
        let query = InventoryQueryBuilder::new()
            .neighbor_states(vec![NeighborState::Stale, NeighborState::Reachable])
            .build();

        let out = merge_devices(sample_neighbors(), sample_leases(), &query);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn merge_devices_uses_ip_observed_id_when_mac_is_absent() {
        let query = InventoryQueryBuilder::new().build();
        let out = merge_devices(
            vec![NeighborEntry {
                ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)),
                dev: Some("br-lan".to_string()),
                mac: None,
                state: NeighborState::Stale,
            }],
            Vec::new(),
            &query,
        );

        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].id,
            Some(DeviceId::Ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20))))
        );
    }

    #[test]
    fn merge_devices_preserves_hook_observation_facts() {
        let query = InventoryQueryBuilder::new().build();
        let out = merge_devices_with_observations(
            Vec::new(),
            Vec::new(),
            vec![DeviceObservationFact {
                kind: "dhcp".into(),
                action: "update".into(),
                mac: Some("aa:bb:cc:dd:ee:ff".parse().expect("mac")),
                ip: Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 30))),
                hostname: Some("lda".into()),
                first_seen_unix: Some(10),
                last_seen_unix: Some(20),
            }],
            &query,
        );

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].names, vec!["lda".to_string()]);
        assert_eq!(
            out[0].id,
            Some(DeviceId::Mac("aa:bb:cc:dd:ee:ff".parse().expect("mac")))
        );
        assert_eq!(out[0].observations.len(), 1);
        assert_eq!(out[0].endpoints.len(), 1);
        assert_eq!(out[0].endpoints[0].key.source, EndpointSource::HookDhcp);
        assert!(out[0].ips.is_empty());
        assert_eq!(out[0].presence, Presence::Unknown);
    }

    #[test]
    fn merge_devices_marks_neigh_remove_observation_offline() {
        let query = InventoryQueryBuilder::new().build();
        let out = merge_devices_with_observations(
            Vec::new(),
            Vec::new(),
            vec![DeviceObservationFact {
                kind: "neigh".into(),
                action: "remove".into(),
                mac: Some("aa:bb:cc:dd:ee:ff".parse().expect("mac")),
                ip: Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 30))),
                hostname: None,
                first_seen_unix: Some(10),
                last_seen_unix: Some(20),
            }],
            &query,
        );

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].presence, Presence::Offline);
        assert_eq!(out[0].endpoints[0].key.source, EndpointSource::HookNeighbor);
        assert!(out[0].ips.is_empty());
    }

    #[test]
    fn merge_devices_keeps_dhcp_remove_observation_unknown() {
        let query = InventoryQueryBuilder::new().build();
        let out = merge_devices_with_observations(
            Vec::new(),
            Vec::new(),
            vec![DeviceObservationFact {
                kind: "dhcp".into(),
                action: "remove".into(),
                mac: Some("aa:bb:cc:dd:ee:ff".parse().expect("mac")),
                ip: Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 30))),
                hostname: None,
                first_seen_unix: Some(10),
                last_seen_unix: Some(20),
            }],
            &query,
        );

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].presence, Presence::Unknown);
        assert_eq!(out[0].endpoints[0].key.source, EndpointSource::HookDhcp);
        assert!(out[0].ips.is_empty());
    }

    #[test]
    fn merge_devices_uses_ip_id_for_ip_only_hook_endpoint_without_summary_ip() {
        let query = InventoryQueryBuilder::new().build();
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 31));
        let out = merge_devices_with_observations(
            Vec::new(),
            Vec::new(),
            vec![DeviceObservationFact {
                kind: "neigh".into(),
                action: "remove".into(),
                mac: None,
                ip: Some(ip),
                hostname: None,
                first_seen_unix: Some(10),
                last_seen_unix: Some(20),
            }],
            &query,
        );

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, Some(DeviceId::Ip(ip)));
        assert!(out[0].ips.is_empty());
        assert_eq!(out[0].endpoints.len(), 1);
    }
}
