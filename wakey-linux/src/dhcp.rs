use std::path::PathBuf;

mod leases;

pub use leases::{
    enrich_leases_with_nud_state, parse_dhcp_lease_line, read_dhcp_leases,
    read_dhcp_leases_from_path, read_dhcp_leases_with_names,
    read_dhcp_leases_with_names_from_paths,
};

const DEFAULT_DHCP_LEASES: &str = "/tmp/dhcp.leases";
const DHCP_LEASES_ENV: &str = "WAKEY_DHCP_LEASES";

pub(crate) fn dhcp_leases_path() -> PathBuf {
    configured_path(DHCP_LEASES_ENV, DEFAULT_DHCP_LEASES)
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
    use std::time::{SystemTime, UNIX_EPOCH};

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

}
