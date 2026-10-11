//! Fleet and single-agent reads share one sequence and a single SQLite read view.

use crate::state::types::*;
use anyhow::{Context, Result};
use sqlx::{SqliteConnection, SqlitePool};

mod assembly;
use assembly::DeviceRows;

/// A read transaction prevents child rows from belonging to a newer snapshot
/// than their parents. Keep scoped SQL explicit so agent reads use indexed filters.
pub(super) async fn load(
    pool: &SqlitePool,
    agent_id: Option<&str>,
) -> Result<Vec<AgentDeviceWithChildren>> {
    let mut tx = pool
        .begin()
        .await
        .context("failed starting device read transaction")?;
    let rows = DeviceRows {
        devices: load_devices(&mut tx, agent_id).await?,
        macs: load_macs(&mut tx, agent_id).await?,
        ips: load_ips(&mut tx, agent_id).await?,
        hostnames: load_hostnames(&mut tx, agent_id).await?,
        endpoints: load_endpoints(&mut tx, agent_id).await?,
        facts: load_facts(&mut tx, agent_id).await?,
    };
    tx.commit()
        .await
        .context("failed completing device read transaction")?;
    Ok(rows.into_devices())
}

async fn load_devices(
    connection: &mut SqliteConnection,
    agent_id: Option<&str>,
) -> Result<Vec<AgentDeviceRow>> {
    match agent_id {
        Some(agent_id) => sqlx::query_as!(
            AgentDeviceRow,
            r#"SELECT agent_id, device_key, presence, display_name,
                    first_seen_unix, last_seen_unix
             FROM agent_devices
             WHERE agent_id = ?1
             ORDER BY device_key"#,
            agent_id,
        )
        .fetch_all(&mut *connection)
        .await
        .context("failed listing agent devices for agent"),
        None => sqlx::query_as!(
            AgentDeviceRow,
            r#"SELECT agent_id, device_key, presence, display_name,
                    first_seen_unix, last_seen_unix
             FROM agent_devices
             ORDER BY agent_id, device_key"#,
        )
        .fetch_all(&mut *connection)
        .await
        .context("failed listing agent devices"),
    }
}

async fn load_macs(
    connection: &mut SqliteConnection,
    agent_id: Option<&str>,
) -> Result<Vec<AgentDeviceMacRow>> {
    match agent_id {
        Some(agent_id) => sqlx::query_as!(
            AgentDeviceMacRow,
            r#"SELECT agent_id as "agent_id!", device_key as "device_key!", mac as "mac!"
             FROM agent_device_macs
             WHERE agent_id = ?1"#,
            agent_id,
        )
        .fetch_all(&mut *connection)
        .await
        .context("failed listing agent device macs for agent"),
        None => sqlx::query_as!(
            AgentDeviceMacRow,
            r#"SELECT agent_id as "agent_id!", device_key as "device_key!", mac as "mac!"
             FROM agent_device_macs"#,
        )
        .fetch_all(&mut *connection)
        .await
        .context("failed listing agent device macs"),
    }
}

async fn load_ips(
    connection: &mut SqliteConnection,
    agent_id: Option<&str>,
) -> Result<Vec<AgentDeviceIpRow>> {
    match agent_id {
        Some(agent_id) => sqlx::query_as!(
            AgentDeviceIpRow,
            r#"SELECT agent_id as "agent_id!", device_key as "device_key!", ip as "ip!"
             FROM agent_device_ips
             WHERE agent_id = ?1"#,
            agent_id,
        )
        .fetch_all(&mut *connection)
        .await
        .context("failed listing agent device ips for agent"),
        None => sqlx::query_as!(
            AgentDeviceIpRow,
            r#"SELECT agent_id as "agent_id!", device_key as "device_key!", ip as "ip!"
             FROM agent_device_ips"#,
        )
        .fetch_all(&mut *connection)
        .await
        .context("failed listing agent device ips"),
    }
}

async fn load_hostnames(
    connection: &mut SqliteConnection,
    agent_id: Option<&str>,
) -> Result<Vec<AgentDeviceHostnameRow>> {
    match agent_id {
        Some(agent_id) => sqlx::query_as!(
            AgentDeviceHostnameRow,
            r#"SELECT agent_id as "agent_id!", device_key as "device_key!", hostname as "hostname!"
             FROM agent_device_hostnames
             WHERE agent_id = ?1"#,
            agent_id,
        )
        .fetch_all(&mut *connection)
        .await
        .context("failed listing agent device hostnames for agent"),
        None => sqlx::query_as!(
            AgentDeviceHostnameRow,
            r#"SELECT agent_id as "agent_id!", device_key as "device_key!", hostname as "hostname!"
             FROM agent_device_hostnames"#,
        )
        .fetch_all(&mut *connection)
        .await
        .context("failed listing agent device hostnames"),
    }
}

async fn load_endpoints(
    connection: &mut SqliteConnection,
    agent_id: Option<&str>,
) -> Result<Vec<AgentDeviceEndpointRow>> {
    match agent_id {
        Some(agent_id) => sqlx::query_as::<_, AgentDeviceEndpointRow>(
            r#"SELECT agent_id, device_key, endpoint_key, source, mac, ip,
                    hostname, interface, presence, first_seen_unix, last_seen_unix
             FROM agent_device_endpoints
             WHERE agent_id = ?1"#,
        )
        .bind(agent_id)
        .fetch_all(&mut *connection)
        .await
        .context("failed listing agent device endpoints for agent"),
        None => sqlx::query_as::<_, AgentDeviceEndpointRow>(
            r#"SELECT agent_id, device_key, endpoint_key, source, mac, ip,
                    hostname, interface, presence, first_seen_unix, last_seen_unix
             FROM agent_device_endpoints"#,
        )
        .fetch_all(&mut *connection)
        .await
        .context("failed listing agent device endpoints"),
    }
}

async fn load_facts(
    connection: &mut SqliteConnection,
    agent_id: Option<&str>,
) -> Result<Vec<AgentDeviceFactRow>> {
    match agent_id {
        Some(agent_id) => {
sqlx::query_as!(
            AgentDeviceFactRow,
            r#"SELECT agent_id as "agent_id!", device_key as "device_key!", fact_json as "fact_json!"
             FROM agent_device_facts
             WHERE agent_id = ?1"#,
            agent_id,
        )
        .fetch_all(&mut *connection)
        .await
        .context("failed listing agent device facts for agent")
        }
        None => {
sqlx::query_as!(
            AgentDeviceFactRow,
            r#"SELECT agent_id as "agent_id!", device_key as "device_key!", fact_json as "fact_json!"
             FROM agent_device_facts"#,
        )
        .fetch_all(&mut *connection)
        .await
        .context("failed listing agent device facts")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::store::helpers::test_helpers::TestStore;
    use wakey_core::{Device, NeighborEntry, NeighborState};

    fn device(ip: &str) -> Device {
        Device::from_parts(
            vec![NeighborEntry {
                ip: ip.parse().unwrap(),
                mac: Some("aa:bb:cc:dd:ee:01".parse().unwrap()),
                dev: Some("br-lan".into()),
                state: NeighborState::Reachable,
            }],
            vec![],
        )
    }

    #[tokio::test]
    async fn scoped_reads_match_fleet_rows_without_leaking_other_agents() {
        let ts = TestStore::new().await;
        let store = ts.store();
        for (agent, ip) in [("a", "192.168.1.10"), ("b", "192.168.1.20")] {
            store
                .replace_agent_device_snapshot(agent, &[device(ip)])
                .await
                .unwrap();
        }
        let fleet = store.list_agent_device_rows().await.unwrap();
        assert_eq!(fleet.len(), 2);
        for agent in ["a", "b", "missing"] {
            let scoped = store.list_agent_device_rows_for_agent(agent).await.unwrap();
            let expected = fleet
                .iter()
                .filter(|row| row.device.agent_id == agent)
                .cloned()
                .collect::<Vec<_>>();
            // Compare every parent/child field, including timestamps and endpoint metadata.
            assert_eq!(format!("{scoped:?}"), format!("{expected:?}"));
        }
    }

    #[tokio::test]
    async fn child_reads_share_the_parent_snapshot_during_a_concurrent_write() {
        let ts = TestStore::new().await;
        let store = ts.store();
        store
            .replace_agent_device_snapshot("a", &[device("192.168.1.10")])
            .await
            .unwrap();
        let mut tx = store.pool.begin().await.unwrap();
        let parents = load_devices(&mut tx, Some("a")).await.unwrap();
        assert_eq!(parents.len(), 1);

        // In WAL mode the writer can commit while the reader still sees the
        // original snapshot. No sleeps or timing assumptions are needed.
        store
            .replace_agent_device_snapshot("a", &[device("192.168.1.20")])
            .await
            .unwrap();
        let ips = load_ips(&mut tx, Some("a")).await.unwrap();
        let endpoints = load_endpoints(&mut tx, Some("a")).await.unwrap();
        assert_eq!(ips[0].ip, "192.168.1.10");
        assert_eq!(endpoints[0].ip.as_deref(), Some("192.168.1.10"));
        tx.commit().await.unwrap();

        let current = store.list_agent_device_rows_for_agent("a").await.unwrap();
        assert_eq!(
            current[0].ips,
            ["192.168.1.20".parse::<std::net::IpAddr>().unwrap()]
        );
    }
}
