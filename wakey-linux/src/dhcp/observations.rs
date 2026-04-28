use std::io::{self, ErrorKind};
use std::net::IpAddr;

use macaddr::MacAddr;
use serde::{Deserialize, Serialize};

use super::{mac_name_cache_path, now_unix, observation_store_path};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalObservationStore {
    #[serde(default)]
    pub dhcp_clients: std::collections::BTreeMap<String, ObservedDhcpClient>,
    #[serde(default)]
    pub neighbors: std::collections::BTreeMap<String, ObservedNeighbor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservedDhcpClient {
    pub mac: String,
    pub ip: Option<IpAddr>,
    pub hostname: Option<String>,
    pub first_seen_unix: u64,
    pub last_seen_unix: u64,
    pub last_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservedNeighbor {
    pub key: String,
    pub mac: Option<String>,
    pub ip: Option<IpAddr>,
    pub first_seen_unix: u64,
    pub last_seen_unix: u64,
    pub last_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalDeviceObservation {
    pub kind: String,
    pub action: String,
    pub mac: Option<String>,
    pub ip: Option<IpAddr>,
    pub hostname: Option<String>,
    pub first_seen_unix: u64,
    pub last_seen_unix: u64,
}

/// Load the MAC-to-name cache used to preserve useful names across lease churn.
pub async fn load_mac_name_cache() -> io::Result<std::collections::BTreeMap<String, String>> {
    load_mac_name_cache_from_path(mac_name_cache_path()).await
}

pub async fn load_mac_name_cache_from_path(
    path: impl AsRef<std::path::Path>,
) -> io::Result<std::collections::BTreeMap<String, String>> {
    match tokio::fs::read_to_string(path).await {
        Ok(s) => serde_json::from_str(&s).map_err(io::Error::other),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Default::default()),
        Err(e) => Err(e),
    }
}

/// Persist the MAC-to-name cache back to disk.
async fn save_mac_name_cache(map: &std::collections::BTreeMap<String, String>) -> io::Result<()> {
    save_mac_name_cache_to_path(mac_name_cache_path(), map).await
}

pub(super) async fn save_mac_name_cache_to_path(
    path: impl AsRef<std::path::Path>,
    map: &std::collections::BTreeMap<String, String>,
) -> io::Result<()> {
    let s = serde_json::to_string(map).map_err(io::Error::other)?;
    let _ = tokio::fs::write(path, s).await;
    Ok(())
}

pub async fn load_observation_store() -> io::Result<LocalObservationStore> {
    match tokio::fs::read_to_string(observation_store_path()).await {
        Ok(s) => serde_json::from_str(&s).map_err(io::Error::other),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Default::default()),
        Err(e) => Err(e),
    }
}

pub async fn load_observation_store_from_path(
    path: impl AsRef<std::path::Path>,
) -> io::Result<LocalObservationStore> {
    match tokio::fs::read_to_string(path).await {
        Ok(s) => serde_json::from_str(&s).map_err(io::Error::other),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Default::default()),
        Err(e) => Err(e),
    }
}

async fn save_observation_store(store: &LocalObservationStore) -> io::Result<()> {
    let s = serde_json::to_string(store).map_err(io::Error::other)?;
    tokio::fs::write(observation_store_path(), s).await
}

pub async fn list_local_observations() -> io::Result<Vec<LocalDeviceObservation>> {
    let store = load_observation_store().await?;
    list_local_observations_from_store(store)
}

pub async fn list_local_observations_from_path(
    path: impl AsRef<std::path::Path>,
) -> io::Result<Vec<LocalDeviceObservation>> {
    let store = load_observation_store_from_path(path).await?;
    list_local_observations_from_store(store)
}

fn list_local_observations_from_store(
    store: LocalObservationStore,
) -> io::Result<Vec<LocalDeviceObservation>> {
    let mut out = Vec::with_capacity(store.dhcp_clients.len() + store.neighbors.len());
    out.extend(
        store
            .dhcp_clients
            .into_values()
            .map(|row| LocalDeviceObservation {
                kind: "dhcp".into(),
                action: row.last_action,
                mac: Some(row.mac),
                ip: row.ip,
                hostname: row.hostname,
                first_seen_unix: row.first_seen_unix,
                last_seen_unix: row.last_seen_unix,
            }),
    );
    out.extend(
        store
            .neighbors
            .into_values()
            .map(|row| LocalDeviceObservation {
                kind: "neigh".into(),
                action: row.last_action,
                mac: row.mac,
                ip: row.ip,
                hostname: None,
                first_seen_unix: row.first_seen_unix,
                last_seen_unix: row.last_seen_unix,
            }),
    );
    out.sort_by(|a, b| {
        b.last_seen_unix
            .cmp(&a.last_seen_unix)
            .then(a.kind.cmp(&b.kind))
            .then(a.mac.cmp(&b.mac))
            .then(a.ip.cmp(&b.ip))
    });
    Ok(out)
}

/// Observe a DHCP hotplug event and update the local MAC-to-name cache.
pub async fn observe_dhcp_event(
    action: &str,
    mac: MacAddr,
    ip: Option<IpAddr>,
    hostname: Option<&str>,
) -> io::Result<bool> {
    if !matches!(action, "add" | "update" | "old" | "remove") {
        // old not emitted by hotplug
        return Ok(false);
    }

    let hostname = hostname
        .map(str::trim)
        .filter(|v| !v.is_empty() && *v != "*")
        .map(ToOwned::to_owned);
    let now = now_unix();
    let mac_s = mac.to_string().to_ascii_lowercase();

    let mut store = load_observation_store().await.unwrap_or_default();
    let mut changed = false;
    store
        .dhcp_clients
        .entry(mac_s.clone())
        .and_modify(|row| {
            if row.ip != ip
                || row.hostname != hostname
                || row.last_action != action
                || row.last_seen_unix != now
            {
                row.ip = ip;
                row.hostname = hostname.clone();
                row.last_action = action.to_string();
                row.last_seen_unix = now;
                changed = true;
            }
        })
        .or_insert_with(|| {
            changed = true;
            ObservedDhcpClient {
                mac: mac_s.clone(),
                ip,
                hostname: hostname.clone(),
                first_seen_unix: now,
                last_seen_unix: now,
                last_action: action.to_string(),
            }
        });
    if changed {
        save_observation_store(&store).await?;
    }

    if let Some(hostname) = hostname {
        let mut cache = load_mac_name_cache().await.unwrap_or_default();
        if cache.get(&mac_s).map(|v| v != &hostname).unwrap_or(true) {
            cache.insert(mac_s, hostname);
            save_mac_name_cache(&cache).await?;
            changed = true;
        }
    }

    Ok(changed)
}

pub async fn observe_neighbor_event(
    action: &str,
    mac: Option<MacAddr>,
    ip: Option<IpAddr>,
) -> io::Result<bool> {
    if !matches!(action, "add" | "update" | "old" | "remove") {
        // update and remove not emitted by hotplug
        return Ok(false);
    }
    let Some(key) = mac
        .map(|value| format!("mac:{}", value.to_string().to_ascii_lowercase()))
        .or_else(|| ip.map(|value| format!("ip:{}", value)))
    else {
        return Ok(false);
    };

    let now = now_unix();
    let mac = mac.map(|value| value.to_string().to_ascii_lowercase());
    let mut store = load_observation_store().await.unwrap_or_default();
    let mut changed = false;
    store
        .neighbors
        .entry(key.clone())
        .and_modify(|row| {
            if row.mac != mac
                || row.ip != ip
                || row.last_action != action
                || row.last_seen_unix != now
            {
                row.mac = mac.clone();
                row.ip = ip;
                row.last_action = action.to_string();
                row.last_seen_unix = now;
                changed = true;
            }
        })
        .or_insert_with(|| {
            changed = true;
            ObservedNeighbor {
                key,
                mac,
                ip,
                first_seen_unix: now,
                last_seen_unix: now,
                last_action: action.to_string(),
            }
        });
    if changed {
        save_observation_store(&store).await?;
    }
    Ok(changed)
}
