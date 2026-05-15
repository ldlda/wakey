use std::io::{self, ErrorKind};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use macaddr::MacAddr;
use serde::{Deserialize, Serialize};

const DEFAULT_MAC_NAME_CACHE: &str = "/tmp/wakey_mac_names.json";
const DEFAULT_OBSERVATION_STORE: &str = "/tmp/wakey_observations.json";
const MAC_NAME_CACHE_ENV: &str = "WAKEY_MAC_NAME_CACHE";
const OBSERVATION_STORE_ENV: &str = "WAKEY_OBSERVATION_STORE";

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(crate) fn mac_name_cache_path() -> PathBuf {
    configured_path(MAC_NAME_CACHE_ENV, DEFAULT_MAC_NAME_CACHE)
}

pub(crate) fn observation_store_path() -> PathBuf {
    configured_path(OBSERVATION_STORE_ENV, DEFAULT_OBSERVATION_STORE)
}

fn configured_path(env_key: &str, default: &str) -> PathBuf {
    std::env::var_os(env_key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

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
    path: impl AsRef<Path>,
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
    path: impl AsRef<Path>,
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
    path: impl AsRef<Path>,
) -> io::Result<LocalObservationStore> {
    match tokio::fs::read_to_string(path).await {
        Ok(s) => serde_json::from_str(&s).map_err(io::Error::other),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Default::default()),
        Err(e) => Err(e),
    }
}

async fn save_observation_store(store: &LocalObservationStore) -> io::Result<()> {
    save_observation_store_to_path(observation_store_path(), store).await
}

async fn save_observation_store_to_path(
    path: impl AsRef<Path>,
    store: &LocalObservationStore,
) -> io::Result<()> {
    let s = serde_json::to_string(store).map_err(io::Error::other)?;
    tokio::fs::write(path, s).await
}

pub async fn list_local_observations() -> io::Result<Vec<LocalDeviceObservation>> {
    let store = load_observation_store().await?;
    Ok(list_local_observations_from_store(store))
}

pub async fn list_local_observations_from_path(
    path: impl AsRef<Path>,
) -> io::Result<Vec<LocalDeviceObservation>> {
    let store = load_observation_store_from_path(path).await?;
    Ok(list_local_observations_from_store(store))
}

pub async fn prune_removed_observations_from_path(path: impl AsRef<Path>) -> io::Result<usize> {
    let path = path.as_ref();
    let mut store = load_observation_store_from_path(path).await?;
    let before = store.dhcp_clients.len() + store.neighbors.len();
    store
        .dhcp_clients
        .retain(|_, row| !row.last_action.eq_ignore_ascii_case("remove"));
    store
        .neighbors
        .retain(|_, row| !row.last_action.eq_ignore_ascii_case("remove"));
    let after = store.dhcp_clients.len() + store.neighbors.len();
    let removed = before.saturating_sub(after);
    if removed > 0 {
        save_observation_store_to_path(path, &store).await?;
    }
    Ok(removed)
}

fn list_local_observations_from_store(store: LocalObservationStore) -> Vec<LocalDeviceObservation> {
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
    out
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
                changed = true; // update row
            }
        })
        .or_insert_with(|| {
            changed = true; // insert new row
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
            changed = true; // update mac -> name cache... 
        }
    }

    Ok(changed)
}

pub async fn observe_neighbor_event(
    action: &str,
    mac: Option<MacAddr>,
    ip: Option<IpAddr>,
) -> io::Result<bool> {
    let action = normalize_neighbor_action(action);
    if !is_neighbor_observation_action(&action) {
        return Ok(false);
    }
    let Some(key) = neighbor_observation_key(mac, ip) else {
        return Ok(false);
    };

    let now = now_unix();
    let mac = mac.map(|value| value.to_string().to_ascii_lowercase());
    let mut store = load_observation_store().await.unwrap_or_default();
    let mut changed = false;
    let alias_keys = neighbor_observation_alias_keys(mac.as_deref(), ip, &key);
    let mut row = store
        .neighbors
        .remove(&key)
        .or_else(|| {
            alias_keys
                .iter()
                .find_map(|alias| store.neighbors.remove(alias))
        })
        .unwrap_or_else(|| {
            changed = true; // append
            ObservedNeighbor {
                key: key.clone(),
                mac: mac.clone(),
                ip,
                first_seen_unix: now,
                last_seen_unix: now,
                last_action: action.clone(),
            }
        });
    for alias in alias_keys {
        if store.neighbors.remove(&alias).is_some() {
            changed = true; // remove old alias
        }
    }
    if row.key != key
        || row.mac != mac
        || row.ip != ip
        || row.last_action != action
        || row.last_seen_unix != now
    {
        row.key = key.clone();
        row.mac = mac.clone();
        row.ip = ip;
        row.last_action = action.clone();
        row.last_seen_unix = now;
        changed = true; // something changed
    }
    store.neighbors.insert(key, row);
    if let (Some(mac), Some(ip)) = (mac.as_deref(), ip)
        && !is_offline_neighbor_action(&action)
    {
        mark_replaced_neighbor_ips_removed(&mut store, mac, ip, now, &mut changed);
    }
    if changed {
        save_observation_store(&store).await?;
    }
    Ok(changed)
}

fn normalize_neighbor_action(action: &str) -> String {
    match action.trim().to_ascii_lowercase().as_str() {
        "del" => "remove".into(),
        "old" => "update".into(),
        other => other.to_string(),
    }
}

fn is_neighbor_observation_action(action: &str) -> bool {
    matches!(action, "add" | "update" | "remove")
}

fn is_offline_neighbor_action(action: &str) -> bool {
    action == "remove"
}

fn neighbor_observation_key(mac: Option<MacAddr>, ip: Option<IpAddr>) -> Option<String> {
    match (mac, ip) {
        (Some(mac), Some(ip)) => Some(format!(
            "mac:{}:ip:{ip}",
            mac.to_string().to_ascii_lowercase()
        )),
        (Some(mac), None) => Some(format!("mac:{}", mac.to_string().to_ascii_lowercase())),
        (None, Some(ip)) => Some(format!("ip:{ip}")),
        (None, None) => None,
    }
}

fn neighbor_observation_alias_keys(
    mac: Option<&str>,
    ip: Option<IpAddr>,
    primary_key: &str,
) -> Vec<String> {
    let mut keys = Vec::with_capacity(2);
    if let Some(mac) = mac {
        keys.push(format!("mac:{mac}"));
    }
    if let Some(ip) = ip {
        keys.push(format!("ip:{ip}"));
    }
    keys.retain(|key| key != primary_key);
    keys
}

fn mark_replaced_neighbor_ips_removed(
    store: &mut LocalObservationStore,
    mac: &str,
    current_ip: IpAddr,
    now: u64,
    changed: &mut bool,
) {
    for row in store.neighbors.values_mut() {
        let Some(row_ip) = row.ip else {
            continue;
        };
        // found another ip for same mac, mark it as removed
        if row.mac.as_deref() == Some(mac)
            && row_ip != current_ip
            && same_ip_family(row_ip, current_ip)
            && !is_offline_neighbor_action(&row.last_action)
        {
            row.last_action = "remove".into();
            row.last_seen_unix = now;
            *changed = true;
        }
    }
}

fn same_ip_family(a: IpAddr, b: IpAddr) -> bool {
    matches!(
        (a, b),
        (IpAddr::V4(_), IpAddr::V4(_)) | (IpAddr::V6(_), IpAddr::V6(_))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    struct EnvGuard {
        keys: Vec<&'static str>, // vec of 1 key
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

    #[tokio::test]
    #[serial]
    async fn neighbor_observations_are_keyed_by_mac_ip_pair() {
        let observation_path = temp_file("neighbor-observations");
        let _observation_guard = EnvGuard::set(OBSERVATION_STORE_ENV, &observation_path);
        let mac = "aa:bb:cc:dd:ee:ff".parse().expect("mac should parse");

        observe_neighbor_event(
            "add",
            Some(mac),
            Some("192.168.1.2".parse().expect("ip should parse")),
        )
        .await
        .expect("first observation should write");
        observe_neighbor_event(
            "update",
            Some(mac),
            Some("192.168.1.3".parse().expect("ip should parse")),
        )
        .await
        .expect("second observation should write");

        let store = load_observation_store()
            .await
            .expect("observation store should read");
        assert!(
            store
                .neighbors
                .contains_key("mac:aa:bb:cc:dd:ee:ff:ip:192.168.1.2")
        );
        assert!(
            store
                .neighbors
                .contains_key("mac:aa:bb:cc:dd:ee:ff:ip:192.168.1.3")
        );
        assert_eq!(
            store.neighbors["mac:aa:bb:cc:dd:ee:ff:ip:192.168.1.2"].last_action,
            "remove"
        );

        let _ = tokio::fs::remove_file(observation_path).await;
    }

    #[tokio::test]
    #[serial]
    async fn neighbor_observation_migrates_coarse_mac_key_to_mac_ip_pair() {
        let observation_path = temp_file("neighbor-observations-migrate");
        let _observation_guard = EnvGuard::set(OBSERVATION_STORE_ENV, &observation_path);
        let mut neighbors = std::collections::BTreeMap::new();
        neighbors.insert(
            "mac:aa:bb:cc:dd:ee:ff".to_string(),
            ObservedNeighbor {
                key: "mac:aa:bb:cc:dd:ee:ff".to_string(),
                mac: Some("aa:bb:cc:dd:ee:ff".to_string()),
                ip: None,
                first_seen_unix: 1,
                last_seen_unix: 1,
                last_action: "add".to_string(),
            },
        );
        let fixture = LocalObservationStore {
            dhcp_clients: Default::default(),
            neighbors,
        };
        tokio::fs::write(
            &observation_path,
            serde_json::to_string(&fixture).expect("fixture should serialize"),
        )
        .await
        .expect("fixture should write");

        observe_neighbor_event(
            "update",
            Some("aa:bb:cc:dd:ee:ff".parse().expect("mac should parse")),
            Some("192.168.1.2".parse().expect("ip should parse")),
        )
        .await
        .expect("observation should write");

        let store = load_observation_store()
            .await
            .expect("observation store should read");
        assert!(!store.neighbors.contains_key("mac:aa:bb:cc:dd:ee:ff"));
        let row = store
            .neighbors
            .get("mac:aa:bb:cc:dd:ee:ff:ip:192.168.1.2")
            .expect("coarse key should migrate to pair key");
        assert_eq!(row.first_seen_unix, 1);
        assert_eq!(row.mac.as_deref(), Some("aa:bb:cc:dd:ee:ff"));
        assert_eq!(
            row.ip
                .expect("ip should be carried into migrated row")
                .to_string(),
            "192.168.1.2"
        );
        assert_eq!(row.last_action, "update");

        let _ = tokio::fs::remove_file(observation_path).await;
    }
}
