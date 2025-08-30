/// MAC->name cache location (ephemeral)
const MAC_NAME_CACHE: &str = "/tmp/wakey_mac_names.json";

/// Load MAC->name cache from disk
async fn load_mac_name_cache() -> io::Result<std::collections::BTreeMap<String, String>> {
    match tokio::fs::read_to_string(MAC_NAME_CACHE).await {
        Ok(s) => serde_json::from_str(&s).map_err(io::Error::other),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Default::default()),
        Err(e) => Err(e),
    }
}

/// Save MAC->name cache to disk
async fn save_mac_name_cache(map: &std::collections::BTreeMap<String, String>) -> io::Result<()> {
    let s = serde_json::to_string(map).map_err(io::Error::other)?;
    let _ = tokio::fs::write(MAC_NAME_CACHE, s).await;
    Ok(())
}

/// Read all leases, filling names from MAC->name cache if missing
pub async fn read_dhcp_leases_with_names() -> io::Result<Vec<DhcpLeaseLine>> {
    let leases = read_dhcp_leases().await?;
    let mut cache = load_mac_name_cache().await.unwrap_or_default();
    let mut changed = false;
    let mut leases_with_names = Vec::with_capacity(leases.len());
    for mut l in leases {
        let mac_s = l.mac.to_string();
        if let Some(ref name) = l.name {
            // if no name in cache file or it changed
            if cache.get(&mac_s).map(|v| v != name).unwrap_or(true) {
                cache.insert(mac_s, name.clone());
                changed = true;
            }
        } else if let Some(prev) = cache.get(&mac_s) {
            l.name = Some(prev.clone());
        }
        leases_with_names.push(l);
    }
    if changed {
        let _ = save_mac_name_cache(&cache).await;
    }
    Ok(leases_with_names)
}
use macaddr::MacAddr;
use serde::Serializer;
use std::io::{self, ErrorKind};
use std::net::IpAddr;

/// A single line from /tmp/dhcp.leases
#[derive(Debug, Clone, serde::Serialize)]
pub struct DhcpLeaseLine {
    /// Epoch seconds when the lease expires
    pub expires_epoch: u64,
    pub ip: IpAddr,
    #[serde(serialize_with = "ser_mac")]
    pub mac: MacAddr,
    pub name: Option<String>,
}

fn ser_mac<S: Serializer>(m: &MacAddr, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&m.to_string())
}

/// Parse one line of /tmp/dhcp.leases
pub fn parse_dhcp_lease_line(line: &str) -> Option<DhcpLeaseLine> {
    let mut c = line.split_whitespace();
    let expires_epoch: u64 = c.next()?.parse().ok()?;
    let mac = c.next()?.parse().ok()?;
    let ip = c.next()?.parse().ok()?;
    let name = c.next().filter(|c| *c != "*").map(str::to_string);
    // ignore any remaining columns (e.g., client-id)
    Some(DhcpLeaseLine {
        expires_epoch,
        ip,
        mac,
        name,
    })
}

/// Read all leases from /tmp/dhcp.leases (simple and fast)
pub async fn read_dhcp_leases() -> io::Result<Vec<DhcpLeaseLine>> {
    match tokio::fs::read_to_string("/tmp/dhcp.leases").await {
        Ok(file) => Ok(file.lines().filter_map(parse_dhcp_lease_line).collect()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}
