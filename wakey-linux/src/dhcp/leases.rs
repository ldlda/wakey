use std::io::{self, ErrorKind};
use std::net::IpAddr;

use wakey_core::{DhcpLease, DhcpLeaseWithState};

use super::{dhcp_leases_path, mac_name_cache_path, observation_store_path};
use crate::dhcp::observations::{
    load_mac_name_cache_from_path, load_observation_store_from_path, save_mac_name_cache_to_path,
};

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
