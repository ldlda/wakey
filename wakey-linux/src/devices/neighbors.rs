use anyhow::{Context, Result};
use futures::future::try_join_all;
use lda_ipjs::subcommands::neighbor;
use std::collections::HashSet;
use std::net::IpAddr;
use wakey_core::{InventoryQuery, NeighborEntry, NeighborState, Query};

/// Resolve a hostname through the local resolver and return all reported IPs.
pub async fn get_ips(machine_name: &str) -> Result<impl Iterator<Item = IpAddr>> {
    Ok(tokio::net::lookup_host((machine_name, 0))
        .await
        .with_context(|| format!("DNS resolve failed for {machine_name}"))?
        .map(|c| c.ip()))
}

/// Query Linux neighbor data and project it into `wakey-core` neighbor rows.
///
/// `machine_names` are resolved first and intersected with any explicit `ips`
/// filter. On Unix this prefers the netlink-backed `ipjs` path; elsewhere it
/// falls back to the JSON command backend.
pub async fn get_neighbors(
    machine_names: &[impl AsRef<str>],
    ips: &[IpAddr],
    devs: &[impl AsRef<str>],
    state: &[NeighborState],
    macs: &[macaddr::MacAddr],
) -> Result<Vec<NeighborEntry>> {
    let resolved_ips: HashSet<IpAddr> = if !machine_names.is_empty() {
        try_join_all(machine_names.iter().map(|n| get_ips(n.as_ref())))
            .await?
            .into_iter()
            .flatten()
            .collect()
    } else {
        HashSet::new()
    };

    let ip_filter: Vec<IpAddr> = if ips.is_empty() && resolved_ips.is_empty() {
        vec![]
    } else if ips.is_empty() {
        resolved_ips.into_iter().collect()
    } else if resolved_ips.is_empty() {
        ips.iter().map(|ip| ip.to_canonical()).collect()
    } else {
        ips.iter()
            .map(|ip| ip.to_canonical())
            .filter(|ip| resolved_ips.contains(ip))
            .collect()
    };

    let nud_filter: Vec<neighbor::NUDState> = state.iter().copied().map(to_ipjs_state).collect();
    let dev_strs: Vec<&str> = devs.iter().map(AsRef::as_ref).collect();

    #[cfg(unix)]
    {
        let results = neighbor::nl::get(&ip_filter, &dev_strs, &nud_filter, macs)
            .await
            .context("rtnetlink failed")?
            .into_iter()
            .map(map_neighbor_item)
            .collect();
        Ok(results)
    }

    #[cfg(not(unix))]
    {
        let mut results = Vec::new();
        if ip_filter.is_empty() {
            let dev = dev_strs.first().copied();
            results = neighbor::get_with_backend(neighbor::Backend::Json, None, dev, &nud_filter)
                .await
                .context("ip -j neigh failed")?
                .into_iter()
                .map(map_neighbor_item)
                .collect();
        } else {
            for ip in &ip_filter {
                let dev = dev_strs.first().copied();
                let mut rows = neighbor::get_with_backend(
                    neighbor::Backend::Json,
                    Some(*ip),
                    dev,
                    &nud_filter,
                )
                .await
                .with_context(|| format!("ip -j neigh failed for {ip}"))?
                .into_iter()
                .map(map_neighbor_item)
                .collect::<Vec<_>>();
                results.append(&mut rows);
            }
        }

        if !dev_strs.is_empty() {
            results.retain(|row| row.dev.as_deref().is_some_and(|d| dev_strs.contains(&d)));
        }
        if !macs.is_empty() {
            let mac_set: HashSet<macaddr::MacAddr> = macs.iter().copied().collect();
            results.retain(|row| row.mac.is_some_and(|m| mac_set.contains(&m)));
        }
        if !ip_filter.is_empty() {
            let ip_set: HashSet<IpAddr> = ip_filter.iter().copied().collect();
            results.retain(|row| ip_set.contains(&row.ip));
        }
        Ok(results)
    }
}

/// Convenience wrapper around [`get_neighbors`] using an `InventoryQuery` filter.
pub async fn query_neighbors(query: &InventoryQuery) -> Result<Vec<NeighborEntry>> {
    let mut names: Vec<&str> = Vec::new();
    let mut ips = Vec::new();
    let mut devs: Vec<&str> = Vec::new();
    let mut nuds = Vec::new();
    let mut macs = Vec::new();

    for term in query {
        match term {
            Query::Text(v) => names.push(v.as_str()),
            Query::Ip(v) => ips.push(*v),
            Query::Interface(v) => devs.push(v.as_str()),
            Query::NeighborState(v) => nuds.push(*v),
            Query::Mac(v) => macs.push(*v),
        }
    }

    get_neighbors(&names, &ips, &devs, &nuds, &macs).await
}

fn to_ipjs_state(value: NeighborState) -> lda_ipjs::subcommands::neighbor::NUDState {
    match value {
        NeighborState::Permanent => lda_ipjs::subcommands::neighbor::NUDState::Permanent,
        NeighborState::Noarp => lda_ipjs::subcommands::neighbor::NUDState::Noarp,
        NeighborState::Reachable => lda_ipjs::subcommands::neighbor::NUDState::Reachable,
        NeighborState::Stale => lda_ipjs::subcommands::neighbor::NUDState::Stale,
        NeighborState::None => lda_ipjs::subcommands::neighbor::NUDState::None,
        NeighborState::Incomplete => lda_ipjs::subcommands::neighbor::NUDState::Incomplete,
        NeighborState::Delay => lda_ipjs::subcommands::neighbor::NUDState::Delay,
        NeighborState::Probe => lda_ipjs::subcommands::neighbor::NUDState::Probe,
        NeighborState::Failed => lda_ipjs::subcommands::neighbor::NUDState::Failed,
    }
}

fn from_ipjs_state(value: lda_ipjs::subcommands::neighbor::NUDState) -> NeighborState {
    match value {
        lda_ipjs::subcommands::neighbor::NUDState::Permanent => NeighborState::Permanent,
        lda_ipjs::subcommands::neighbor::NUDState::Noarp => NeighborState::Noarp,
        lda_ipjs::subcommands::neighbor::NUDState::Reachable => NeighborState::Reachable,
        lda_ipjs::subcommands::neighbor::NUDState::Stale => NeighborState::Stale,
        lda_ipjs::subcommands::neighbor::NUDState::None => NeighborState::None,
        lda_ipjs::subcommands::neighbor::NUDState::Incomplete => NeighborState::Incomplete,
        lda_ipjs::subcommands::neighbor::NUDState::Delay => NeighborState::Delay,
        lda_ipjs::subcommands::neighbor::NUDState::Probe => NeighborState::Probe,
        lda_ipjs::subcommands::neighbor::NUDState::Failed => NeighborState::Failed,
        lda_ipjs::subcommands::neighbor::NUDState::Other(_) => NeighborState::None,
    }
}

fn map_neighbor_item(
    lda_ipjs::subcommands::neighbor::NeighborItem {
        ip,
        dev,
        mac,
        state,
    }: lda_ipjs::subcommands::neighbor::NeighborItem,
) -> NeighborEntry {
    NeighborEntry {
        ip,
        dev,
        mac,
        state: state
            .into_iter()
            .map(from_ipjs_state)
            .max()
            .unwrap_or(NeighborState::None),
    }
}
