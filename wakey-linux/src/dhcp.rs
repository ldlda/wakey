use std::io::{self, ErrorKind};
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use macaddr::MacAddr;
use serde::{Deserialize, Serialize};
use wakey_core::{DhcpLease, DhcpLeaseWithState};

const DEFAULT_DHCP_LEASES: &str = "/tmp/dhcp.leases";
const DEFAULT_MAC_NAME_CACHE: &str = "/tmp/wakey_mac_names.json";
const DEFAULT_OBSERVATION_STORE: &str = "/tmp/wakey_observations.json";
const DHCP_LEASES_ENV: &str = "WAKEY_DHCP_LEASES";
const MAC_NAME_CACHE_ENV: &str = "WAKEY_MAC_NAME_CACHE";
const OBSERVATION_STORE_ENV: &str = "WAKEY_OBSERVATION_STORE";

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

async fn save_mac_name_cache_to_path(
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

/// Parse one `dnsmasq`-style DHCP lease line.
pub fn parse_dhcp_lease_line(line: &str) -> Option<DhcpLease> {
    let mut c = line.split_whitespace();
    let expires_epoch: u64 = c.next()?.parse().ok()?;
    let mac = c.next()?.parse().ok()?;
    let ip = c.next()?.parse().ok()?;
    let name = c.next().filter(|c| *c != "*").map(str::to_string);
    Some(DhcpLease {
        expires_epoch,
        ip,
        mac,
        name,
    })
}

/// Read raw DHCP leases from the configured dnsmasq lease file.
pub async fn read_dhcp_leases() -> io::Result<Vec<DhcpLease>> {
    read_dhcp_leases_from_path(dhcp_leases_path()).await
}

pub async fn read_dhcp_leases_from_path(
    path: impl AsRef<std::path::Path>,
) -> io::Result<Vec<DhcpLease>> {
    match tokio::fs::read_to_string(path).await {
        Ok(file) => Ok(file.lines().filter_map(parse_dhcp_lease_line).collect()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Read DHCP leases and fill missing names from the MAC-name cache.
pub async fn read_dhcp_leases_with_names() -> io::Result<Vec<DhcpLease>> {
    read_dhcp_leases_with_names_from_paths(
        dhcp_leases_path(),
        observation_store_path(),
        mac_name_cache_path(),
    )
    .await
}

pub async fn read_dhcp_leases_with_names_from_paths(
    leases_path: impl AsRef<std::path::Path>,
    observation_store_path: impl AsRef<std::path::Path>,
    mac_name_cache_path: impl AsRef<std::path::Path>,
) -> io::Result<Vec<DhcpLease>> {
    let leases = read_dhcp_leases_from_path(leases_path).await?;
    let observations = load_observation_store_from_path(observation_store_path)
        .await
        .unwrap_or_default();
    let mac_name_cache_path = mac_name_cache_path.as_ref();
    let mut cache = load_mac_name_cache_from_path(mac_name_cache_path)
        .await
        .unwrap_or_default();
    let mut changed = false;
    let mut leases_with_names = Vec::with_capacity(leases.len());
    for mut l in leases {
        let mac_s = l.mac.to_string();
        if let Some(ref name) = l.name {
            if cache.get(&mac_s).map(|v| v != name).unwrap_or(true) {
                cache.insert(mac_s, name.clone());
                changed = true;
            }
        } else if let Some(prev) = observations
            .dhcp_clients
            .get(&mac_s)
            .and_then(|row| row.hostname.as_ref())
        {
            l.name = Some(prev.clone());
        } else if let Some(prev) = cache.get(&mac_s) {
            l.name = Some(prev.clone());
        }
        leases_with_names.push(l);
    }
    if changed {
        let _ = save_mac_name_cache_to_path(mac_name_cache_path, &cache).await;
    }
    Ok(leases_with_names)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn dhcp_leases_path() -> PathBuf {
    configured_path(DHCP_LEASES_ENV, DEFAULT_DHCP_LEASES)
}

fn mac_name_cache_path() -> PathBuf {
    configured_path(MAC_NAME_CACHE_ENV, DEFAULT_MAC_NAME_CACHE)
}

fn observation_store_path() -> PathBuf {
    configured_path(OBSERVATION_STORE_ENV, DEFAULT_OBSERVATION_STORE)
}

fn configured_path(env_key: &str, default: &str) -> PathBuf {
    std::env::var_os(env_key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

/// Enrich DHCP leases with the best currently known neighbor state per IP.
pub async fn enrich_leases_with_nud_state(leases: Vec<DhcpLease>) -> Vec<DhcpLeaseWithState> {
    let ips: Vec<IpAddr> = leases.iter().map(|l| l.ip).collect();
    let mut map: std::collections::HashMap<IpAddr, wakey_core::NeighborState> =
        std::collections::HashMap::new();
    if let Ok(rows) =
        crate::devices::get_neighbors(&[] as &[&str], &ips, &[] as &[&str], &[], &[]).await
    {
        for row in rows {
            let state = row.state;
            let r = state.rank();
            map.entry(row.ip)
                .and_modify(|e| {
                    if r > e.rank() {
                        *e = state
                    }
                })
                .or_insert(state);
        }
    }
    leases
        .into_iter()
        .map(|lease_line| DhcpLeaseWithState {
            nud_state: map.get(&lease_line.ip).copied(),
            lease_line,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    struct EnvGuard {
        keys: Vec<&'static str>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &std::path::Path) -> Self {
            let guard = Self { keys: vec![key] };
            // SAFETY: these tests are serialized and do not spawn work that reads these
            // environment variables outside the test body.
            unsafe {
                std::env::set_var(key, value);
            }
            guard
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for key in &self.keys {
                // SAFETY: these tests are serialized and do not spawn work that reads these
                // environment variables outside the test body.
                unsafe {
                    std::env::remove_var(key);
                }
            }
        }
    }

    fn temp_file(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("wakey-linux-{name}-{}-{nonce}", std::process::id()))
    }

    #[tokio::test]
    #[serial]
    async fn dhcp_lease_file_path_can_be_overridden() {
        let path = temp_file("leases");
        let _guard = EnvGuard::set(DHCP_LEASES_ENV, &path);
        tokio::fs::write(&path, "1893456000 aa:bb:cc:dd:ee:ff 192.168.1.2 lda *\n")
            .await
            .expect("lease fixture should write");

        let leases = read_dhcp_leases().await.expect("leases should read");
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].name.as_deref(), Some("lda"));
        assert_eq!(leases[0].ip.to_string(), "192.168.1.2");

        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    #[serial]
    async fn observation_and_name_cache_paths_can_be_overridden() {
        let observation_path = temp_file("observations");
        let cache_path = temp_file("names");
        let _observation_guard = EnvGuard::set(OBSERVATION_STORE_ENV, &observation_path);
        let _cache_guard = EnvGuard::set(MAC_NAME_CACHE_ENV, &cache_path);

        let changed = observe_dhcp_event(
            "add",
            "aa:bb:cc:dd:ee:ff".parse().expect("mac should parse"),
            Some("192.168.1.2".parse().expect("ip should parse")),
            Some("lda"),
        )
        .await
        .expect("observation should write");
        assert!(changed);

        let store = load_observation_store()
            .await
            .expect("observation store should read");
        assert!(store.dhcp_clients.contains_key("aa:bb:cc:dd:ee:ff"));

        let cache = load_mac_name_cache().await.expect("name cache should read");
        assert_eq!(cache.get("aa:bb:cc:dd:ee:ff"), Some(&"lda".to_string()));

        let _ = tokio::fs::remove_file(observation_path).await;
        let _ = tokio::fs::remove_file(cache_path).await;
    }
}
