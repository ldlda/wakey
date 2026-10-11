//! Replaces snapshot rows using the caller's transaction. No helper commits.

use anyhow::{Context, Result};
use sqlx::SqliteConnection;
use wakey_core::{Device, DeviceEndpoint};

use super::{
    endpoint_presence, endpoint_source_to_str, endpoint_storage_key, endpoints_for_storage,
    presence_to_str,
};

/// Includes key lookup, normalization, parent/child writes and missing-device
/// deletion. This measures replacement work, not pure SQL execution time.
#[tracing::instrument(name = "db_snapshot_write_rows", skip_all, fields(devices = devices.len()))]
pub(super) async fn replace(
    connection: &mut SqliteConnection,
    agent_id: &str,
    devices: &[Device],
) -> Result<usize> {
    let mut incoming_keys = std::collections::HashSet::with_capacity(devices.len());
    let snapshot_time = super::now_unix();

    let existing_keys: Vec<String> = sqlx::query_scalar!(
        "SELECT device_key FROM agent_devices WHERE agent_id = ?1",
        agent_id
    )
    .fetch_all(&mut *connection)
    .await
    .context("failed fetching existing keys")?;

    for device in devices {
        let Some(device_id) = &device.id else {
            continue;
        };
        let device_key = super::device_key_from_id(device_id);
        incoming_keys.insert(device_key.clone());

        write_device(connection, agent_id, &device_key, device, snapshot_time).await?;
    }

    for old_key in existing_keys {
        if !incoming_keys.contains(&old_key) {
            sqlx::query!(
                "DELETE FROM agent_devices WHERE agent_id = ?1 AND device_key = ?2",
                agent_id,
                old_key
            )
            .execute(&mut *connection)
            .await
            .context("failed pruning old agent device")?;
        }
    }
    Ok(incoming_keys.len())
}

async fn write_device(
    connection: &mut SqliteConnection,
    agent_id: &str,
    device_key: &str,
    device: &Device,
    snapshot_time: u64,
) -> Result<()> {
    let endpoints = endpoints_for_storage(device);
    let presence = presence_to_str(endpoint_presence(&endpoints, device.presence));
    upsert_parent(connection, agent_id, device_key, presence, snapshot_time).await?;
    replace_macs(connection, agent_id, device_key, device).await?;
    replace_ips(connection, agent_id, device_key, device).await?;
    replace_endpoints(connection, agent_id, device_key, &endpoints, snapshot_time).await?;
    replace_hostnames(connection, agent_id, device_key, device).await?;
    replace_facts(connection, agent_id, device_key, device).await
}

/// Updates snapshot inclusion time, while preserving the parent's first-seen time.
async fn upsert_parent(
    connection: &mut SqliteConnection,
    agent_id: &str,
    device_key: &str,
    presence: &str,
    snapshot_time: u64,
) -> Result<()> {
    let snapshot_time_i64 = i64::try_from(snapshot_time).context("snapshot time overflow")?;
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
    .execute(&mut *connection)
    .await
    .context("failed upserting agent device")?;
    Ok(())
}

async fn replace_macs(
    connection: &mut SqliteConnection,
    agent_id: &str,
    device_key: &str,
    device: &Device,
) -> Result<()> {
    sqlx::query!(
        "DELETE FROM agent_device_macs WHERE agent_id = ?1 AND device_key = ?2",
        agent_id,
        device_key
    )
    .execute(&mut *connection)
    .await
    .context("failed deleting device macs")?;

    if !device.macs.is_empty() {
        let mut builder =
            sqlx::QueryBuilder::new("INSERT INTO agent_device_macs (agent_id, device_key, mac) ");
        builder.push_values(&device.macs, |mut b, mac| {
            b.push_bind(agent_id)
                .push_bind(device_key)
                .push_bind(mac.to_string().to_ascii_lowercase());
        });
        builder
            .build()
            .execute(&mut *connection)
            .await
            .context("failed inserting device macs")?;
    }
    Ok(())
}

async fn replace_ips(
    connection: &mut SqliteConnection,
    agent_id: &str,
    device_key: &str,
    device: &Device,
) -> Result<()> {
    sqlx::query!(
        "DELETE FROM agent_device_ips WHERE agent_id = ?1 AND device_key = ?2",
        agent_id,
        device_key
    )
    .execute(&mut *connection)
    .await
    .context("failed deleting device ips")?;

    if !device.ips.is_empty() {
        let mut builder =
            sqlx::QueryBuilder::new("INSERT INTO agent_device_ips (agent_id, device_key, ip) ");
        builder.push_values(&device.ips, |mut b, ip| {
            b.push_bind(agent_id)
                .push_bind(device_key)
                .push_bind(ip.to_string());
        });
        builder
            .build()
            .execute(&mut *connection)
            .await
            .context("failed inserting device ips")?;
    }
    Ok(())
}

async fn replace_endpoints(
    connection: &mut SqliteConnection,
    agent_id: &str,
    device_key: &str,
    endpoints: &[DeviceEndpoint],
    snapshot_time: u64,
) -> Result<()> {
    sqlx::query("DELETE FROM agent_device_endpoints WHERE agent_id = ?1 AND device_key = ?2")
        .bind(agent_id)
        .bind(device_key)
        .execute(&mut *connection)
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
                .push_bind(device_key)
                .push_bind(endpoint_storage_key(endpoint))
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
            .execute(&mut *connection)
            .await
            .context("failed inserting device endpoints")?;
    }
    Ok(())
}

async fn replace_hostnames(
    connection: &mut SqliteConnection,
    agent_id: &str,
    device_key: &str,
    device: &Device,
) -> Result<()> {
    sqlx::query!(
        "DELETE FROM agent_device_hostnames WHERE agent_id = ?1 AND device_key = ?2",
        agent_id,
        device_key
    )
    .execute(&mut *connection)
    .await
    .context("failed deleting device hostnames")?;

    if !device.names.is_empty() {
        let mut builder = sqlx::QueryBuilder::new(
            "INSERT INTO agent_device_hostnames (agent_id, device_key, hostname) ",
        );
        builder.push_values(&device.names, |mut b, hostname| {
            b.push_bind(agent_id)
                .push_bind(device_key)
                .push_bind(hostname);
        });
        builder
            .build()
            .execute(&mut *connection)
            .await
            .context("failed inserting device hostnames")?;
    }
    Ok(())
}

async fn replace_facts(
    connection: &mut SqliteConnection,
    agent_id: &str,
    device_key: &str,
    device: &Device,
) -> Result<()> {
    sqlx::query!(
        "DELETE FROM agent_device_facts WHERE agent_id = ?1 AND device_key = ?2",
        agent_id,
        device_key
    )
    .execute(&mut *connection)
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
            b.push_bind(agent_id).push_bind(device_key).push_bind(fact);
        });
        builder
            .build()
            .execute(&mut *connection)
            .await
            .context("failed inserting device facts")?;
    }
    Ok(())
}
