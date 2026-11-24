use std::{path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use tracing::{Instrument, debug, warn};
use wakey_core::{
    Device, DeviceInventory, DeviceObservationFact, DhcpLease, DhcpLeaseWithState, InventoryQuery,
    NeighborEntry, Presence, Query,
};

use crate::service::leases::leases_without_state;
use crate::service::query::resolve_query;

/// Paths for the three local inventory sources and optional hook retention.
pub struct InventorySources {
    pub dhcp_leases_path: PathBuf,
    pub observation_store_path: PathBuf,
    pub mac_name_cache_path: PathBuf,
    /// Set by the agent to prune hooks before reading them. The CLI leaves them alone.
    pub observation_retention: Option<Duration>,
}

impl InventorySources {
    /// Use the standalone CLI's environment overrides and platform defaults.
    pub fn from_env() -> Self {
        Self {
            dhcp_leases_path: wakey_linux::dhcp::dhcp_leases_path(),
            observation_store_path: wakey_linux::observations::observation_store_path(),
            mac_name_cache_path: wakey_linux::observations::mac_name_cache_path(),
            observation_retention: None,
        }
    }

    /// Read DHCP leases, using these sources' hook and cache files for names.
    pub async fn read_leases(&self) -> std::io::Result<Vec<DhcpLease>> {
        wakey_linux::dhcp::read_dhcp_leases_with_names_from_paths(
            &self.dhcp_leases_path,
            &self.observation_store_path,
            &self.mac_name_cache_path,
        )
        .await
    }

    /// Merge neighbors, named DHCP leases, and hook facts into a local inventory.
    #[tracing::instrument(name = "inventory", skip_all, fields(neighbors = tracing::field::Empty, leases = tracing::field::Empty, observations = tracing::field::Empty, devices = tracing::field::Empty))]
    pub async fn collect(&self, query: InventoryQuery) -> Result<DeviceInventory> {
        let span = tracing::Span::current();
        let neighbors = wakey_linux::devices::query_neighbors(&query)
            .instrument(tracing::info_span!("inventory_neighbors"))
            .await?;
        span.record("neighbors", neighbors.len());
        let leases = self
            .read_leases()
            .instrument(tracing::info_span!("inventory_dhcp_leases"))
            .await
            .with_context(|| {
                format!(
                    "failed to read DHCP leases from {}",
                    self.dhcp_leases_path.display()
                )
            })?;
        span.record("leases", leases.len());
        // Lease naming can use the hook cache before old hook facts are pruned.
        let observations = self.read_hook_facts().await;
        span.record("observations", observations.len());
        let devices = merge_devices_with_observations(
            neighbors,
            leases_without_state(leases),
            observations,
            &query,
        );
        span.record("devices", devices.len());
        debug!(devices = devices.len(), "merged device inventory");
        Ok(DeviceInventory { devices })
    }

    async fn read_hook_facts(&self) -> Vec<DeviceObservationFact> {
        if let Some(retention) = self.observation_retention {
            match wakey_linux::observations::prune_expired(&self.observation_store_path, retention)
                .instrument(tracing::info_span!("inventory_observation_prune"))
                .await
            {
                Ok(removed) if removed > 0 => {
                    debug!(removed, "pruned stale local hook observations")
                }
                Ok(_) => {}
                Err(err) => warn!(error = %err, "failed pruning stale local hook observations"),
            }
        }
        // Hook storage is best effort. Keep neighbors and leases if this read fails.
        match wakey_linux::observations::list_local_observations_from_path(
            &self.observation_store_path,
        )
        .instrument(tracing::info_span!("inventory_hook_store"))
        .await
        {
            Ok(observations) => observations
                .into_iter()
                .filter_map(local_observation_to_fact)
                .collect(),
            Err(err) => {
                warn!(error = %err, "failed reading local hook observations for inventory");
                Vec::new()
            }
        }
    }

    pub(super) async fn resolve_devices(&self, input: impl Into<String>) -> Result<Vec<Device>> {
        let query = resolve_query(input).await?;
        self.collect(query).await.map(|inventory| inventory.devices)
    }
}

/// Resolve free-form input and return merged devices rather than raw source rows.
pub async fn resolve_devices(input: impl Into<String>) -> Result<Vec<Device>> {
    InventorySources::from_env().resolve_devices(input).await
}

/// Collect local inventory using the standalone CLI's paths.
pub async fn inventory(query: InventoryQuery) -> Result<DeviceInventory> {
    InventorySources::from_env().collect(query).await
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
#[tracing::instrument(name = "inventory_merge", skip_all, fields(neighbors = neighbors.len(), leases = leases.len(), observations = observations.len()))]
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
    // Match the core signal order: Unknown has no verdict, while Offline
    // carries concrete evidence and belongs above it in the inventory list.
    match presence {
        Presence::Online => 3,
        Presence::LikelyOnline => 2,
        Presence::Offline => 1,
        Presence::Unknown => 0,
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
    fn inventory_sorts_offline_before_unknown() {
        let neighbors = [NeighborState::Incomplete, NeighborState::Failed]
            .into_iter()
            .enumerate()
            .map(|(index, state)| NeighborEntry {
                ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, index as u8 + 10)),
                dev: None,
                mac: None,
                state,
            })
            .collect();
        let devices = merge_devices(neighbors, Vec::new(), &InventoryQueryBuilder::new().build());
        assert_eq!(devices[0].presence, Presence::Offline);
        assert_eq!(devices[1].presence, Presence::Unknown);
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
