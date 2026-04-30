use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

mod leases;
mod observations;

pub use leases::{
    enrich_leases_with_nud_state, parse_dhcp_lease_line, read_dhcp_leases,
    read_dhcp_leases_from_path, read_dhcp_leases_with_names,
    read_dhcp_leases_with_names_from_paths,
};
pub use observations::{
    LocalDeviceObservation, LocalObservationStore, ObservedDhcpClient, ObservedNeighbor,
    list_local_observations, list_local_observations_from_path, load_mac_name_cache,
    load_mac_name_cache_from_path, load_observation_store, load_observation_store_from_path,
    observe_dhcp_event, observe_neighbor_event,
};

const DEFAULT_DHCP_LEASES: &str = "/tmp/dhcp.leases";
const DEFAULT_MAC_NAME_CACHE: &str = "/tmp/wakey_mac_names.json";
const DEFAULT_OBSERVATION_STORE: &str = "/tmp/wakey_observations.json";
const DHCP_LEASES_ENV: &str = "WAKEY_DHCP_LEASES";
const MAC_NAME_CACHE_ENV: &str = "WAKEY_MAC_NAME_CACHE";
const OBSERVATION_STORE_ENV: &str = "WAKEY_OBSERVATION_STORE";

pub(crate) fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(crate) fn dhcp_leases_path() -> PathBuf {
    configured_path(DHCP_LEASES_ENV, DEFAULT_DHCP_LEASES)
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

    #[tokio::test]
    #[serial]
    async fn neighbor_observations_are_keyed_by_mac_ip_pair() {
        let observation_path = temp_file("neighbor-observations");
        let _observation_guard = EnvGuard::set(OBSERVATION_STORE_ENV, &observation_path);
        let mac = "aa:bb:cc:dd:ee:ff".parse().expect("mac should parse");

        observe_neighbor_event(
            "stale",
            Some(mac),
            Some("192.168.1.2".parse().expect("ip should parse")),
        )
        .await
        .expect("first observation should write");
        observe_neighbor_event(
            "reachable",
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
}
