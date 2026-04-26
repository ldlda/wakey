use std::io::{self, ErrorKind};
use std::net::IpAddr;

use macaddr::MacAddr;
use wakey_core::{DhcpLease, DhcpLeaseWithState};

const MAC_NAME_CACHE: &str = "/tmp/wakey_mac_names.json";

/// Load the MAC-to-name cache used to preserve useful names across lease churn.
pub async fn load_mac_name_cache() -> io::Result<std::collections::BTreeMap<String, String>> {
    match tokio::fs::read_to_string(MAC_NAME_CACHE).await {
        Ok(s) => serde_json::from_str(&s).map_err(io::Error::other),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Default::default()),
        Err(e) => Err(e),
    }
}

/// Persist the MAC-to-name cache back to disk.
async fn save_mac_name_cache(map: &std::collections::BTreeMap<String, String>) -> io::Result<()> {
    let s = serde_json::to_string(map).map_err(io::Error::other)?;
    let _ = tokio::fs::write(MAC_NAME_CACHE, s).await;
    Ok(())
}

/// Observe a DHCP hotplug event and update the local MAC-to-name cache.
pub async fn observe_dhcp_event(
    action: &str,
    mac: MacAddr,
    _ip: Option<IpAddr>,
    hostname: Option<&str>,
) -> io::Result<bool> {
    if !matches!(action, "add" | "update" | "old") {
        return Ok(false);
    }

    let Some(hostname) = hostname
        .map(str::trim)
        .filter(|v| !v.is_empty() && *v != "*")
    else {
        return Ok(false);
    };

    let mut cache = load_mac_name_cache().await.unwrap_or_default();
    let mac_s = mac.to_string();
    if cache.get(&mac_s).map(|v| v == hostname).unwrap_or(false) {
        return Ok(false);
    }

    cache.insert(mac_s, hostname.to_string());
    save_mac_name_cache(&cache).await?;
    Ok(true)
}

/// Observe a neighbor hotplug event. This is currently a no-op placeholder for
/// keeping DHCP and neighbor hook commands symmetrical.
pub async fn observe_neighbor_event(
    _action: &str,
    _mac: Option<MacAddr>,
    _ip: Option<IpAddr>,
) -> io::Result<bool> {
    Ok(false)
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

/// Read raw DHCP leases from `/tmp/dhcp.leases`.
pub async fn read_dhcp_leases() -> io::Result<Vec<DhcpLease>> {
    match tokio::fs::read_to_string("/tmp/dhcp.leases").await {
        Ok(file) => Ok(file.lines().filter_map(parse_dhcp_lease_line).collect()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Read DHCP leases and fill missing names from the MAC-name cache.
pub async fn read_dhcp_leases_with_names() -> io::Result<Vec<DhcpLease>> {
    let leases = read_dhcp_leases().await?;
    let mut cache = load_mac_name_cache().await.unwrap_or_default();
    let mut changed = false;
    let mut leases_with_names = Vec::with_capacity(leases.len());
    for mut l in leases {
        let mac_s = l.mac.to_string();
        if let Some(ref name) = l.name {
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
