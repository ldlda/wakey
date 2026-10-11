use anyhow::{Context, Result};
use wakey_core::{Device, DeviceEndpoint, DeviceId, EndpointSource};

use super::Store;
use super::helpers::core::*;
use crate::state::types::*;

impl Store {
    /// Expire agent-reported devices, preserving all operator-saved identities.
    /// `last_seen_unix` on the parent is CC snapshot inclusion time, not proof
    /// of online presence. Cascades remove child addresses, endpoints and facts.
    #[tracing::instrument(name = "db_prune_devices", skip_all)]
    pub async fn prune_devices(&self, retention: std::time::Duration) -> Result<u64> {
        self.prune_devices_at(retention, now_unix()).await
    }

    async fn prune_devices_at(&self, retention: std::time::Duration, now: u64) -> Result<u64> {
        if retention.is_zero() {
            return Ok(0);
        }
        let cutoff = i64::try_from(now.saturating_sub(retention.as_secs()))
            .context("device retention cutoff overflow")?;
        let mut tx = self.begin_write().await?;
        let removed = sqlx::query!(
            "DELETE FROM agent_devices WHERE last_seen_unix < ?1",
            cutoff
        )
        .execute(&mut *tx)
        .await
        .context("failed pruning expired agent devices")?
        .rows_affected();
        tx.commit()
            .await
            .context("failed committing device retention cleanup")?;
        if removed > 0 {
            self.touch_fleet();
        }
        Ok(removed)
    }

    /// Replace the complete device snapshot for an agent.
    ///
    /// This is the only write path for agent device state. Every sync source
    /// (WebSocket snapshot, fleet refresh, HTTP upload) must go through here.
    #[tracing::instrument(name = "db_replace_agent_snapshot", skip_all, fields(devices = devices.len(), accepted = tracing::field::Empty))]
    pub async fn replace_agent_device_snapshot(
        &self,
        agent_id: &str,
        devices: &[Device],
    ) -> Result<usize> {
        let mut tx = self.begin_write().await?;

        let mut incoming_keys = std::collections::HashSet::with_capacity(devices.len());
        let snapshot_time = now_unix();
        let snapshot_time_i64 = i64::try_from(snapshot_time).context("snapshot time overflow")?;

        let existing_keys: Vec<String> = sqlx::query_scalar!(
            "SELECT device_key FROM agent_devices WHERE agent_id = ?1",
            agent_id
        )
        .fetch_all(&mut *tx)
        .await
        .context("failed fetching existing keys")?;

        for device in devices {
            let Some(device_id) = &device.id else {
                continue;
            };
            let device_key = device_key_from_id(device_id);
            incoming_keys.insert(device_key.clone());

            let endpoints = endpoints_for_storage(device);
            let presence = presence_to_str(endpoint_presence(&endpoints, device.presence));
            let display_name: Option<String> = None;

            sqlx::query!(
                "INSERT INTO agent_devices (agent_id, device_key, presence, display_name, first_seen_unix, last_seen_unix)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (agent_id, device_key) DO UPDATE SET
                    presence = excluded.presence,
                    display_name = excluded.display_name,
                    last_seen_unix = excluded.last_seen_unix",
                agent_id,
                device_key,
                presence,
                display_name,
                snapshot_time_i64,
                snapshot_time_i64
            )
            .execute(&mut *tx)
            .await
            .context("failed upserting agent device")?;

            // Replace child rows.
            sqlx::query!(
                "DELETE FROM agent_device_macs WHERE agent_id = ?1 AND device_key = ?2",
                agent_id,
                device_key
            )
            .execute(&mut *tx)
            .await
            .context("failed deleting device macs")?;

            if !device.macs.is_empty() {
                let mut builder = sqlx::QueryBuilder::new(
                    "INSERT INTO agent_device_macs (agent_id, device_key, mac) ",
                );
                builder.push_values(&device.macs, |mut b, mac| {
                    b.push_bind(agent_id)
                        .push_bind(device_key.clone())
                        .push_bind(mac.to_string().to_ascii_lowercase());
                });
                builder
                    .build()
                    .execute(&mut *tx)
                    .await
                    .context("failed inserting device macs")?;
            }

            sqlx::query!(
                "DELETE FROM agent_device_ips WHERE agent_id = ?1 AND device_key = ?2",
                agent_id,
                device_key
            )
            .execute(&mut *tx)
            .await
            .context("failed deleting device ips")?;

            if !device.ips.is_empty() {
                let mut builder = sqlx::QueryBuilder::new(
                    "INSERT INTO agent_device_ips (agent_id, device_key, ip) ",
                );
                builder.push_values(&device.ips, |mut b, ip| {
                    b.push_bind(agent_id)
                        .push_bind(device_key.clone())
                        .push_bind(ip.to_string());
                });
                builder
                    .build()
                    .execute(&mut *tx)
                    .await
                    .context("failed inserting device ips")?;
            }

            sqlx::query(
                "DELETE FROM agent_device_endpoints WHERE agent_id = ?1 AND device_key = ?2",
            )
            .bind(agent_id)
            .bind(&device_key)
            .execute(&mut *tx)
            .await
            .context("failed deleting device endpoints")?;

            if !endpoints.is_empty() {
                let mut builder = sqlx::QueryBuilder::new(
                    "INSERT INTO agent_device_endpoints \
                     (agent_id, device_key, endpoint_key, source, mac, ip, hostname, interface, presence, first_seen_unix, last_seen_unix) ",
                );
                builder.push_values(endpoints, |mut b, endpoint| {
                    let first_seen = endpoint.first_seen_unix.unwrap_or(snapshot_time);
                    let last_seen = endpoint.last_seen_unix.unwrap_or(snapshot_time);
                    b.push_bind(agent_id)
                        .push_bind(device_key.clone())
                        .push_bind(endpoint_storage_key(&endpoint))
                        .push_bind(endpoint_source_to_str(endpoint.key.source))
                        .push_bind(
                            endpoint
                                .key
                                .mac
                                .map(|mac| mac.to_string().to_ascii_lowercase()),
                        )
                        .push_bind(endpoint.key.ip.map(|ip| ip.to_string()))
                        .push_bind(endpoint.hostname.clone())
                        .push_bind(endpoint.interface.clone())
                        .push_bind(presence_to_str(endpoint.presence))
                        .push_bind(i64::try_from(first_seen).unwrap_or(i64::MAX))
                        .push_bind(i64::try_from(last_seen).unwrap_or(i64::MAX));
                });
                builder
                    .build()
                    .execute(&mut *tx)
                    .await
                    .context("failed inserting device endpoints")?;
            }

            sqlx::query!(
                "DELETE FROM agent_device_hostnames WHERE agent_id = ?1 AND device_key = ?2",
                agent_id,
                device_key
            )
            .execute(&mut *tx)
            .await
            .context("failed deleting device hostnames")?;

            if !device.names.is_empty() {
                let mut builder = sqlx::QueryBuilder::new(
                    "INSERT INTO agent_device_hostnames (agent_id, device_key, hostname) ",
                );
                builder.push_values(&device.names, |mut b, hostname| {
                    b.push_bind(agent_id)
                        .push_bind(device_key.clone())
                        .push_bind(hostname);
                });
                builder
                    .build()
                    .execute(&mut *tx)
                    .await
                    .context("failed inserting device hostnames")?;
            }

            sqlx::query!(
                "DELETE FROM agent_device_facts WHERE agent_id = ?1 AND device_key = ?2",
                agent_id,
                device_key
            )
            .execute(&mut *tx)
            .await
            .context("failed deleting device facts")?;

            if !device.observations.is_empty() {
                let mut facts_json = Vec::with_capacity(device.observations.len());
                for obs in &device.observations {
                    facts_json.push(serde_json::to_string(obs).context("failed serializing fact")?);
                }
                let mut builder = sqlx::QueryBuilder::new(
                    "INSERT INTO agent_device_facts (agent_id, device_key, fact_json) ",
                );
                builder.push_values(facts_json, |mut b, fact| {
                    b.push_bind(agent_id)
                        .push_bind(device_key.clone())
                        .push_bind(fact);
                });
                builder
                    .build()
                    .execute(&mut *tx)
                    .await
                    .context("failed inserting device facts")?;
            }
        }

        for old_key in existing_keys {
            if !incoming_keys.contains(&old_key) {
                sqlx::query!(
                    "DELETE FROM agent_devices WHERE agent_id = ?1 AND device_key = ?2",
                    agent_id,
                    old_key
                )
                .execute(&mut *tx)
                .await
                .context("failed pruning old agent device")?;
            }
        }

        tx.commit()
            .await
            .context("failed committing device snapshot")?;
        self.touch_fleet();

        tracing::Span::current().record("accepted", incoming_keys.len());
        Ok(incoming_keys.len())
    }

    /// List all agent device rows with their child MAC/IP/hostname/fact rows.
    #[tracing::instrument(name = "db_list_fleet_devices", skip_all)]
    pub async fn list_agent_device_rows(&self) -> Result<Vec<AgentDeviceWithChildren>> {
        let devices = sqlx::query_as!(
            AgentDeviceRow,
            r#"SELECT agent_id, device_key, presence, display_name,
                    first_seen_unix, last_seen_unix
             FROM agent_devices
             ORDER BY agent_id, device_key"#,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed listing agent devices")?;

        let macs = sqlx::query_as!(
            AgentDeviceMacRow,
            r#"SELECT agent_id as "agent_id!", device_key as "device_key!", mac as "mac!"
             FROM agent_device_macs"#,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed listing agent device macs")?;

        let ips = sqlx::query_as!(
            AgentDeviceIpRow,
            r#"SELECT agent_id as "agent_id!", device_key as "device_key!", ip as "ip!"
             FROM agent_device_ips"#,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed listing agent device ips")?;

        let hostnames = sqlx::query_as!(
            AgentDeviceHostnameRow,
            r#"SELECT agent_id as "agent_id!", device_key as "device_key!", hostname as "hostname!"
             FROM agent_device_hostnames"#,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed listing agent device hostnames")?;

        let endpoints = sqlx::query_as::<_, AgentDeviceEndpointRow>(
            r#"SELECT agent_id, device_key, endpoint_key, source, mac, ip,
                    hostname, interface, presence, first_seen_unix, last_seen_unix
             FROM agent_device_endpoints"#,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed listing agent device endpoints")?;

        let facts = sqlx::query_as!(
            AgentDeviceFactRow,
            r#"SELECT agent_id as "agent_id!", device_key as "device_key!", fact_json as "fact_json!"
             FROM agent_device_facts"#,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed listing agent device facts")?;

        Ok(assemble_device_rows(
            devices, macs, ips, hostnames, endpoints, facts,
        ))
    }

    /// List agent device rows for a single agent.
    #[allow(dead_code)]
    #[tracing::instrument(name = "db_list_agent_devices", skip_all)]
    pub async fn list_agent_device_rows_for_agent(
        &self,
        agent_id: &str,
    ) -> Result<Vec<AgentDeviceWithChildren>> {
        let devices = sqlx::query_as!(
            AgentDeviceRow,
            r#"SELECT agent_id, device_key, presence, display_name,
                    first_seen_unix, last_seen_unix
             FROM agent_devices
             WHERE agent_id = ?1
             ORDER BY device_key"#,
            agent_id,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed listing agent devices for agent")?;

        let macs = sqlx::query_as!(
            AgentDeviceMacRow,
            r#"SELECT agent_id as "agent_id!", device_key as "device_key!", mac as "mac!"
             FROM agent_device_macs
             WHERE agent_id = ?1"#,
            agent_id,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed listing agent device macs for agent")?;

        let ips = sqlx::query_as!(
            AgentDeviceIpRow,
            r#"SELECT agent_id as "agent_id!", device_key as "device_key!", ip as "ip!"
             FROM agent_device_ips
             WHERE agent_id = ?1"#,
            agent_id,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed listing agent device ips for agent")?;

        let hostnames = sqlx::query_as!(
            AgentDeviceHostnameRow,
            r#"SELECT agent_id as "agent_id!", device_key as "device_key!", hostname as "hostname!"
             FROM agent_device_hostnames
             WHERE agent_id = ?1"#,
            agent_id,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed listing agent device hostnames for agent")?;

        let endpoints = sqlx::query_as::<_, AgentDeviceEndpointRow>(
            r#"SELECT agent_id, device_key, endpoint_key, source, mac, ip,
                    hostname, interface, presence, first_seen_unix, last_seen_unix
             FROM agent_device_endpoints
             WHERE agent_id = ?1"#,
        )
        .bind(agent_id)
        .fetch_all(&self.pool)
        .await
        .context("failed listing agent device endpoints for agent")?;

        let facts = sqlx::query_as!(
            AgentDeviceFactRow,
            r#"SELECT agent_id as "agent_id!", device_key as "device_key!", fact_json as "fact_json!"
             FROM agent_device_facts
             WHERE agent_id = ?1"#,
            agent_id,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed listing agent device facts for agent")?;

        Ok(assemble_device_rows(
            devices, macs, ips, hostnames, endpoints, facts,
        ))
    }
}

fn assemble_device_rows(
    devices: Vec<AgentDeviceRow>,
    macs: Vec<AgentDeviceMacRow>,
    ips: Vec<AgentDeviceIpRow>,
    hostnames: Vec<AgentDeviceHostnameRow>,
    endpoints: Vec<AgentDeviceEndpointRow>,
    facts: Vec<AgentDeviceFactRow>,
) -> Vec<AgentDeviceWithChildren> {
    use std::collections::BTreeMap;

    let mut mac_map: BTreeMap<(&str, &str), Vec<&AgentDeviceMacRow>> = BTreeMap::new();
    for row in &macs {
        mac_map
            .entry((row.agent_id.as_str(), row.device_key.as_str()))
            .or_default()
            .push(row);
    }
    let mut ip_map: BTreeMap<(&str, &str), Vec<&AgentDeviceIpRow>> = BTreeMap::new();
    for row in &ips {
        ip_map
            .entry((row.agent_id.as_str(), row.device_key.as_str()))
            .or_default()
            .push(row);
    }
    let mut hostname_map: BTreeMap<(&str, &str), Vec<String>> = BTreeMap::new();
    for row in &hostnames {
        hostname_map
            .entry((row.agent_id.as_str(), row.device_key.as_str()))
            .or_default()
            .push(row.hostname.clone());
    }
    let mut endpoint_map: BTreeMap<(&str, &str), Vec<DeviceEndpoint>> = BTreeMap::new();
    for row in &endpoints {
        if let Some(endpoint) = row.to_endpoint() {
            endpoint_map
                .entry((row.agent_id.as_str(), row.device_key.as_str()))
                .or_default()
                .push(endpoint);
        }
    }
    let mut fact_map: BTreeMap<(&str, &str), Vec<String>> = BTreeMap::new();
    for row in &facts {
        fact_map
            .entry((row.agent_id.as_str(), row.device_key.as_str()))
            .or_default()
            .push(row.fact_json.clone());
    }

    devices
        .into_iter()
        .map(|device| {
            let key = (device.agent_id.as_str(), device.device_key.as_str());
            let macs: Vec<macaddr::MacAddr> = mac_map
                .get(&key)
                .into_iter()
                .flatten()
                .filter_map(|row| match macaddr::MacAddr::try_from(*row) {
                    Ok(mac) => Some(mac),
                    Err(e) => {
                        ::tracing::warn!(error = %e, agent_id = %device.agent_id, device_key = %device.device_key, raw_mac = %row.mac, "failed to parse mac from agent_device_macs row");
                        None
                    }
                })
                .collect();
            let ips: Vec<std::net::IpAddr> = ip_map
                .get(&key)
                .into_iter()
                .flatten()
                .filter_map(|row| match std::net::IpAddr::try_from(*row) {
                    Ok(ip) => Some(ip),
                    Err(e) => {
                        ::tracing::warn!(error = %e, agent_id = %device.agent_id, device_key = %device.device_key, raw_ip = %row.ip, "failed to parse ip from agent_device_ips row");
                        None
                    }
                })
                .collect();
            let hostnames = hostname_map.get(&key).cloned().unwrap_or_default();
            let endpoints = endpoint_map.get(&key).cloned().unwrap_or_default();
            let facts = fact_map.get(&key).cloned().unwrap_or_default();
            AgentDeviceWithChildren {
                macs,
                ips,
                hostnames,
                endpoints,
                facts,
                device,
            }
        })
        .collect()
}

pub fn device_key_from_id(device_id: &DeviceId) -> String {
    match device_id {
        DeviceId::Mac(mac) => format!("mac:{}", mac.to_string().to_ascii_lowercase()),
        DeviceId::Ip(ip) => format!("ip:{ip}"),
    }
}

fn presence_to_str(presence: wakey_core::Presence) -> &'static str {
    match presence {
        wakey_core::Presence::Online => "online",
        wakey_core::Presence::LikelyOnline => "likely_online",
        wakey_core::Presence::Unknown => "unknown",
        wakey_core::Presence::Offline => "offline",
    }
}

fn endpoint_source_to_str(source: EndpointSource) -> &'static str {
    match source {
        EndpointSource::Neighbor => "neighbor",
        EndpointSource::DhcpLease => "dhcp_lease",
        EndpointSource::HookNeighbor => "hook_neighbor",
        EndpointSource::HookDhcp => "hook_dhcp",
    }
}

fn endpoint_storage_key(endpoint: &DeviceEndpoint) -> String {
    format!(
        "{}|{}|{}",
        endpoint_source_to_str(endpoint.key.source),
        endpoint
            .key
            .mac
            .map(|mac| mac.to_string().to_ascii_lowercase())
            .unwrap_or_default(),
        endpoint.key.ip.map(|ip| ip.to_string()).unwrap_or_default()
    )
}

fn endpoints_for_storage(device: &Device) -> Vec<DeviceEndpoint> {
    if !device.endpoints.is_empty() {
        return device.endpoints.clone();
    }

    let mut endpoints = Vec::new();
    for neighbor in &device.neighbors {
        endpoints.push(DeviceEndpoint {
            key: wakey_core::EndpointKey {
                source: EndpointSource::Neighbor,
                mac: neighbor.mac,
                ip: Some(neighbor.ip),
            },
            hostname: None,
            interface: neighbor.dev.clone(),
            presence: wakey_core::Presence::from(neighbor.state),
            first_seen_unix: None,
            last_seen_unix: None,
        });
    }
    for lease in &device.leases {
        endpoints.push(DeviceEndpoint {
            key: wakey_core::EndpointKey {
                source: EndpointSource::DhcpLease,
                mac: Some(lease.mac),
                ip: Some(lease.ip),
            },
            hostname: lease.name.clone(),
            interface: None,
            presence: wakey_core::Presence::Unknown,
            first_seen_unix: None,
            last_seen_unix: None,
        });
    }
    endpoints
}

#[cfg(test)]
mod tests {
    use super::super::helpers::test_helpers::TestStore;
    use std::time::Duration;
    use wakey_core::{
        Device, DeviceId, DeviceObservationFact, NeighborEntry, NeighborState, Presence,
    };

    fn sample_device(mac: &str, ip: &str, name: &str) -> Device {
        Device {
            id: Some(DeviceId::Mac(mac.parse().expect("mac"))),
            names: vec![name.to_string()],
            ips: vec![ip.parse().expect("ip")],
            macs: vec![mac.parse().expect("mac")],
            interfaces: vec!["br-lan".to_string()],
            endpoints: vec![],
            neighbors: vec![NeighborEntry {
                ip: ip.parse().expect("ip"),
                dev: Some("br-lan".to_string()),
                mac: Some(mac.parse().expect("mac")),
                state: NeighborState::Reachable,
            }],
            leases: vec![],
            observations: vec![],
            presence: Presence::Online,
        }
    }

    #[tokio::test]
    async fn device_retention_uses_snapshot_age_and_preserves_saved_identity() {
        use crate::state::types::{DeviceIdentifierInput, KnownDeviceInput};

        let ts = TestStore::new().await;
        let store = ts.store();
        let mut device = sample_device("aa:bb:cc:dd:ee:01", "192.168.1.10", "pc");
        device.neighbors[0].state = NeighborState::Failed;
        device.observations.push(DeviceObservationFact {
            kind: "neigh".into(),
            action: "remove".into(),
            mac: device.macs.first().copied(),
            ip: device.ips.first().copied(),
            hostname: None,
            first_seen_unix: Some(1),
            last_seen_unix: Some(1),
        });
        let saved = store
            .create_known_device(KnownDeviceInput {
                display_name: "saved pc".into(),
                pinned: true,
                notes: None,
                identifiers: vec![DeviceIdentifierInput {
                    kind: "mac".into(),
                    value: "aa:bb:cc:dd:ee:01".into(),
                }],
            })
            .await
            .unwrap();

        // Explicit timestamps exercise the exact boundary and future rows
        // without sleeping or relying on the test machine's wall clock.
        for (agent, seen) in [("expired", 99_i64), ("boundary", 100), ("fresh", 101)] {
            store
                .replace_agent_device_snapshot(agent, &[device.clone()])
                .await
                .unwrap();
            sqlx::query("UPDATE agent_devices SET last_seen_unix = ?1 WHERE agent_id = ?2")
                .bind(seen)
                .bind(agent)
                .execute(&store.pool)
                .await
                .unwrap();
            sqlx::query("UPDATE agent_device_endpoints SET last_seen_unix = 1 WHERE agent_id = ?1")
                .bind(agent)
                .execute(&store.pool)
                .await
                .unwrap();
        }
        let before_cleanup = store.fleet_revision();
        assert_eq!(
            store.prune_devices_at(Duration::ZERO, 1_000).await.unwrap(),
            0
        );
        assert_eq!(store.fleet_revision(), before_cleanup);
        assert_eq!(
            store
                .prune_devices_at(Duration::from_secs(900), 1_000)
                .await
                .unwrap(),
            1
        );
        let after_cleanup = store.fleet_revision();
        assert_ne!(after_cleanup, before_cleanup);
        assert_eq!(
            store
                .prune_devices_at(Duration::from_secs(900), 1_000)
                .await
                .unwrap(),
            0
        );
        assert_eq!(store.fleet_revision(), after_cleanup);

        for table in [
            "agent_devices",
            "agent_device_macs",
            "agent_device_ips",
            "agent_device_hostnames",
            "agent_device_endpoints",
            "agent_device_facts",
        ] {
            let count: i64 = sqlx::query_scalar(&format!(
                "SELECT COUNT(*) FROM {table} WHERE agent_id = 'expired'"
            ))
            .fetch_one(&store.pool)
            .await
            .unwrap();
            assert_eq!(count, 0, "expired rows remain in {table}");
        }
        for agent in ["boundary", "fresh"] {
            let rows = store.list_agent_device_rows_for_agent(agent).await.unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].presence(), Presence::Offline);
        }
        let retained = store
            .get_known_device(&saved.device_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retained.identifiers.len(), 1);
        assert!(retained.pinned);
        // Retention longer than the current epoch must not wrap the cutoff.
        assert_eq!(
            store
                .prune_devices_at(Duration::from_secs(2_000), 1_000)
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn a_new_snapshot_renews_device_retention_even_when_offline() {
        let ts = TestStore::new().await;
        let store = ts.store();
        let mut device = sample_device("aa:bb:cc:dd:ee:01", "192.168.1.10", "pc");
        device.neighbors[0].state = NeighborState::Failed;
        store
            .replace_agent_device_snapshot("agent-a", &[device.clone()])
            .await
            .unwrap();
        sqlx::query("UPDATE agent_devices SET last_seen_unix = 1")
            .execute(&store.pool)
            .await
            .unwrap();
        store
            .replace_agent_device_snapshot("agent-a", &[device])
            .await
            .unwrap();
        let rows = store
            .list_agent_device_rows_for_agent("agent-a")
            .await
            .unwrap();
        let included_at = u64::try_from(rows[0].device.last_seen_unix).unwrap();
        assert_eq!(
            store
                .prune_devices_at(Duration::from_secs(60), included_at + 60)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .prune_devices_at(Duration::from_secs(60), included_at + 61)
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn snapshot_presence_is_derived_from_stored_endpoints() {
        let ts = TestStore::new().await;
        let mut device = sample_device("aa:bb:cc:dd:ee:01", "192.168.1.10", "pc");
        // Compatibility snapshots can lack endpoints but carry raw neighbors.
        device.presence = Presence::Unknown;
        device.neighbors[0].state = NeighborState::Failed;
        ts.store()
            .replace_agent_device_snapshot("agent-a", &[device.clone()])
            .await
            .unwrap();
        let rows = ts
            .store()
            .list_agent_device_rows_for_agent("agent-a")
            .await
            .unwrap();
        assert_eq!(rows[0].device.presence, "offline");
        assert_eq!(rows[0].endpoints[0].presence, Presence::Offline);

        device.presence = Presence::Online;
        device.endpoints = rows[0].endpoints.clone();
        device.endpoints[0].presence = Presence::Unknown;
        ts.store()
            .replace_agent_device_snapshot("agent-a", &[device])
            .await
            .unwrap();
        let rows = ts
            .store()
            .list_agent_device_rows_for_agent("agent-a")
            .await
            .unwrap();
        assert_eq!(rows[0].device.presence, "unknown");
        assert_eq!(rows[0].endpoints[0].presence, Presence::Unknown);
    }

    #[tokio::test]
    async fn snapshot_inserts_devices_and_children() {
        let ts = TestStore::new().await;

        let devices = vec![sample_device(
            "aa:bb:cc:dd:ee:01",
            "192.168.1.10",
            "first-pc",
        )];
        let count = ts
            .store()
            .replace_agent_device_snapshot("agent-a", &devices)
            .await
            .expect("snapshot should succeed");
        assert_eq!(count, 1);

        let rows = ts
            .store()
            .list_agent_device_rows_for_agent("agent-a")
            .await
            .expect("list should succeed");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].device.device_key, "mac:aa:bb:cc:dd:ee:01");
        assert_eq!(rows[0].device.presence, "online");
        assert_eq!(
            rows[0].macs,
            vec!["aa:bb:cc:dd:ee:01".parse::<macaddr::MacAddr>().unwrap()]
        );
        assert_eq!(
            rows[0].ips,
            vec!["192.168.1.10".parse::<std::net::IpAddr>().unwrap()]
        );
        assert_eq!(rows[0].hostnames, vec!["first-pc"]);
        assert_eq!(rows[0].endpoints.len(), 1);
        assert_eq!(
            rows[0].endpoints[0].key.source,
            wakey_core::EndpointSource::Neighbor
        );
        assert_eq!(
            rows[0].endpoints[0].key.ip,
            Some("192.168.1.10".parse().expect("ip"))
        );
    }

    #[tokio::test]
    async fn second_snapshot_prunes_missing_devices() {
        let ts = TestStore::new().await;

        let first = vec![
            sample_device("aa:bb:cc:dd:ee:01", "192.168.1.10", "first"),
            sample_device("aa:bb:cc:dd:ee:02", "192.168.1.11", "second"),
        ];
        ts.store()
            .replace_agent_device_snapshot("agent-a", &first)
            .await
            .expect("first snapshot should succeed");

        let second = vec![sample_device("aa:bb:cc:dd:ee:01", "192.168.1.10", "first")];
        ts.store()
            .replace_agent_device_snapshot("agent-a", &second)
            .await
            .expect("second snapshot should succeed");

        let rows = ts
            .store()
            .list_agent_device_rows_for_agent("agent-a")
            .await
            .expect("list should succeed");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].device.device_key, "mac:aa:bb:cc:dd:ee:01");
    }

    #[tokio::test]
    async fn empty_snapshot_clears_devices() {
        let ts = TestStore::new().await;

        let devices = vec![sample_device("aa:bb:cc:dd:ee:01", "192.168.1.10", "first")];
        ts.store()
            .replace_agent_device_snapshot("agent-a", &devices)
            .await
            .expect("snapshot should succeed");

        ts.store()
            .replace_agent_device_snapshot("agent-a", &[])
            .await
            .expect("empty snapshot should succeed");

        let rows = ts
            .store()
            .list_agent_device_rows_for_agent("agent-a")
            .await
            .expect("list should succeed");
        assert_eq!(rows.len(), 0);
    }

    #[tokio::test]
    async fn first_seen_survives_snapshot_update() {
        let ts = TestStore::new().await;

        let devices = vec![sample_device("aa:bb:cc:dd:ee:01", "192.168.1.10", "first")];
        ts.store()
            .replace_agent_device_snapshot("agent-a", &devices)
            .await
            .expect("first snapshot should succeed");

        let first_seen = ts
            .store()
            .list_agent_device_rows_for_agent("agent-a")
            .await
            .expect("list should succeed")[0]
            .device
            .first_seen_unix;

        tokio::time::sleep(Duration::from_secs(1)).await;

        ts.store()
            .replace_agent_device_snapshot("agent-a", &devices)
            .await
            .expect("second snapshot should succeed");

        let rows = ts
            .store()
            .list_agent_device_rows_for_agent("agent-a")
            .await
            .expect("list should succeed");
        assert_eq!(rows[0].device.first_seen_unix, first_seen);
    }

    #[tokio::test]
    async fn snapshot_does_not_prune_other_agents() {
        let ts = TestStore::new().await;

        let devices_a = vec![sample_device("aa:bb:cc:dd:ee:01", "192.168.1.10", "first")];
        let devices_b = vec![sample_device("aa:bb:cc:dd:ee:02", "192.168.1.11", "second")];
        ts.store()
            .replace_agent_device_snapshot("agent-a", &devices_a)
            .await
            .expect("snapshot a should succeed");
        ts.store()
            .replace_agent_device_snapshot("agent-b", &devices_b)
            .await
            .expect("snapshot b should succeed");

        ts.store()
            .replace_agent_device_snapshot("agent-a", &[])
            .await
            .expect("clear a should succeed");

        let rows_b = ts
            .store()
            .list_agent_device_rows_for_agent("agent-b")
            .await
            .expect("list b should succeed");
        assert_eq!(rows_b.len(), 1);
        assert_eq!(rows_b[0].device.device_key, "mac:aa:bb:cc:dd:ee:02");
    }

    #[tokio::test]
    async fn device_without_id_is_skipped() {
        let ts = TestStore::new().await;

        let devices = vec![Device {
            id: None,
            names: vec!["no-id".to_string()],
            ips: vec![],
            macs: vec![],
            interfaces: vec![],
            endpoints: vec![],
            neighbors: vec![],
            leases: vec![],
            observations: vec![],
            presence: Presence::Unknown,
        }];
        let count = ts
            .store()
            .replace_agent_device_snapshot("agent-a", &devices)
            .await
            .expect("snapshot should succeed");
        assert_eq!(count, 0);

        let rows = ts
            .store()
            .list_agent_device_rows_for_agent("agent-a")
            .await
            .expect("list should succeed");
        assert_eq!(rows.len(), 0);
    }

    #[tokio::test]
    async fn facts_are_stored_as_json() {
        let ts = TestStore::new().await;

        let devices = vec![Device {
            id: Some(DeviceId::Mac("aa:bb:cc:dd:ee:01".parse().expect("mac"))),
            names: vec!["pc".to_string()],
            ips: vec!["192.168.1.10".parse().expect("ip")],
            macs: vec!["aa:bb:cc:dd:ee:01".parse().expect("mac")],
            interfaces: vec![],
            endpoints: vec![],
            neighbors: vec![],
            leases: vec![],
            observations: vec![DeviceObservationFact {
                kind: "dhcp".to_string(),
                action: "update".to_string(),
                mac: Some("aa:bb:cc:dd:ee:01".parse().expect("mac")),
                ip: Some("192.168.1.10".parse().expect("ip")),
                hostname: Some("pc".to_string()),
                first_seen_unix: Some(10),
                last_seen_unix: Some(20),
            }],
            presence: Presence::LikelyOnline,
        }];
        ts.store()
            .replace_agent_device_snapshot("agent-a", &devices)
            .await
            .expect("snapshot should succeed");

        let rows = ts
            .store()
            .list_agent_device_rows_for_agent("agent-a")
            .await
            .expect("list should succeed");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].facts.len(), 1);
        assert!(rows[0].facts[0].contains("dhcp"));
    }
}
